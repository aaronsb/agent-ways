//! One focus owner and the keyboard's ring (ADR-405, #931 increments 5 and
//! 6), through the shell's real key and mouse handlers: the ring's order,
//! every way out of the compose box, Enter opening each element's menu, the
//! single keys the key help names working outside the box, the footer's
//! keys per focus, the operator view and the reserved tabs at the bar's
//! right end, the entry dialog that replaced the lent compose box, and Add
//! agent to chat. In its own file, so its own fixture: alpha and beta live,
//! alpha and the operator in #deploy, an empty #infra.

mod support;

use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;
use attend_chat::signal::{Channel, Signal};
use attend_chat::tabs::Tab;

const ALPHA_SID: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const BETA_SID: &str = "bbbbbbbb-0000-4000-8000-000000000002";
const W: u16 = 120;
const H: u16 = 35;

fn chat() -> Chat {
    support::fixture("focus", |base| {
        support::pin_alive(ALPHA_SID);
        support::pin_alive(BETA_SID);
        let alpha = attend_groups::Groups::new(base, ALPHA_SID);
        alpha.create("deploy", Some("rollout")).unwrap();
        alpha.join("deploy", true).unwrap();
        attend_groups::Groups::new(base, &attend_chat::signal::human_member_id()).join("deploy", true).unwrap();
        attend_groups::Groups::new(base, "").create("infra", None).unwrap();
    });
    let mut c = Chat::new(None, Palette::default(), Shape::PLAIN).sidebar(true).heartbeat(false).enhanced(true).dry_run(true);
    for (i, (sid, cwd)) in [(ALPHA_SID, "/work/alpha"), (BETA_SID, "/work/beta")].into_iter().enumerate() {
        c.push(Signal {
            id: format!("t-{i}"),
            from: format!("claude:{sid}"),
            project: cwd.rsplit('/').next().unwrap().into(),
            cwd: cwd.into(),
            reply_to: None,
            message: format!("message {i} in the feed"),
            ts: 1_790_949_000 + i as u64,
            channel: Channel::Open,
        });
    }
    c
}

fn rows(c: &mut Chat) -> Vec<String> {
    testkit::rows(&testkit::render_screen(c, W, H))
}

fn screen(c: &mut Chat) -> String {
    rows(c).join("\n")
}

fn bar(c: &mut Chat) -> String {
    rows(c).pop().unwrap()
}

/// Draw, then send the key: the real loop draws between keys.
fn press(c: &mut Chat, code: KeyCode) -> bool {
    key(c, KeyEvent::new(code, KeyModifiers::NONE))
}

fn key(c: &mut Chat, k: KeyEvent) -> bool {
    testkit::render_screen(c, W, H);
    testkit::drive(c, &[k])
}

fn typed(c: &mut Chat, s: &str) {
    for ch in s.chars() {
        press(c, KeyCode::Char(ch));
    }
}

/// What has the focus, as the screen shows it: the tab behind the bar's
/// `▸`, the column row behind its `▸`, else the bottom bar's lozenge.
fn focused(c: &mut Chat) -> String {
    let rows = rows(c);
    if let Some(i) = rows[0].find('▸') {
        return tab_at(&rows[0][i + '▸'.len_utf8()..]);
    }
    for r in &rows[1..rows.len() - 1] {
        if let Some(rest) = r.strip_prefix('▸') {
            let row = rest.split('│').next().unwrap();
            let name = row.split_whitespace().find(|w| w.starts_with('#')).or_else(|| row.split_whitespace().find(|w| w.chars().next().is_some_and(char::is_alphanumeric))).unwrap_or("");
            return format!("row {name}");
        }
    }
    let b = rows.last().unwrap().trim_start().to_string();
    format!("mode {}", b.split_whitespace().next().unwrap())
}

/// The tab label at the start of `s`: its number dropped.
fn tab_at(s: &str) -> String {
    let mut words = s.split_whitespace();
    let first = words.next().unwrap_or("");
    let label = if first.chars().all(|c| c.is_ascii_digit()) { words.next().unwrap_or("") } else { first };
    format!("tab {label}")
}

/// The ring from the focused element on, forward, until `stop` is focused.
fn walk(c: &mut Chat, code: KeyCode, stop: &str) -> Vec<String> {
    let mut out = Vec::new();
    for _ in 0..40 {
        press(c, code);
        let f = focused(c);
        out.push(f.clone());
        if f == stop {
            return out;
        }
    }
    panic!("never reached {stop}: {out:?}");
}

#[test]
fn the_ring_walks_the_tab_bar_in_order_then_the_column_the_feed_and_the_box() {
    let mut c = chat();
    assert_eq!(focused(&mut c), "mode chat", "the chat opens typing");
    // ↓ at the end of a box with text leaves it forward, round to the bar.
    press(&mut c, KeyCode::Char('x'));
    press(&mut c, KeyCode::Down);
    let ring = {
        let mut r = vec![focused(&mut c)];
        r.extend(walk(&mut c, KeyCode::Tab, "mode chat"));
        r
    };
    let tabs: Vec<&str> = ring.iter().filter(|f| f.starts_with("tab ")).map(String::as_str).collect();
    assert_eq!(tabs, ["tab ≡", "tab #deploy", "tab #infra", "tab +", "tab #open", "tab ⋯all"], "{ring:?}");
    let rows: Vec<&str> = ring.iter().filter(|f| f.starts_with("row ")).map(String::as_str).collect();
    let channels: Vec<&str> = rows.iter().copied().filter(|r| r.starts_with("row #")).collect();
    let mut sorted = channels.clone();
    sorted.sort();
    assert_eq!(sorted, ["row #deploy", "row #infra", "row #open"], "every channel: {ring:?}");
    let first_channel = rows.iter().position(|r| r.starts_with("row #")).unwrap();
    assert!(rows[..first_channel].len() >= 2 && rows[first_channel..].iter().all(|r| r.starts_with("row #")), "every agent, then every channel: {ring:?}");
    let tail: Vec<&str> = ring.iter().rev().take(2).rev().map(String::as_str).collect();
    assert_eq!(tail, ["mode feed", "mode chat"], "the feed, then the box: {ring:?}");
    // Back the other way: Shift+Tab and ← walk it in reverse.
    press(&mut c, KeyCode::BackTab);
    assert_eq!(focused(&mut c), "mode feed");
    press(&mut c, KeyCode::Left);
    assert!(focused(&mut c).starts_with("row "), "{}", focused(&mut c));
    press(&mut c, KeyCode::Right);
    press(&mut c, KeyCode::Right);
    assert_eq!(focused(&mut c), "mode chat", "→ forward, round to the box");
}

#[test]
fn every_way_out_of_the_compose_box() {
    // Shift+Tab leaves backward, to the feed.
    let mut c = chat();
    press(&mut c, KeyCode::BackTab);
    assert_eq!(focused(&mut c), "mode feed");
    // ↑ at the start of the first line leaves backward; before that it moves.
    let mut c = chat();
    typed(&mut c, "hello");
    press(&mut c, KeyCode::Up);
    assert_eq!((focused(&mut c), c.input().cursor()), ("mode chat".to_string(), 0), "↑ on the first line goes to its start");
    press(&mut c, KeyCode::Up);
    assert_eq!(focused(&mut c), "mode feed", "↑ at the start of the first line leaves");
    assert_eq!(c.input().text(), "hello", "the draft stays");
    // ↓ at the end of the last line leaves forward, round to the tab bar.
    let mut c = chat();
    typed(&mut c, "one");
    key(&mut c, KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
    typed(&mut c, "two");
    press(&mut c, KeyCode::Up);
    assert_eq!((focused(&mut c), c.input().cursor()), ("mode chat".to_string(), 3), "↑ from the second line moves to the first");
    press(&mut c, KeyCode::Left);
    press(&mut c, KeyCode::Down);
    assert_eq!(c.input().cursor(), 6, "↓ to the line below, the same column");
    press(&mut c, KeyCode::Down);
    assert_eq!(focused(&mut c), "mode chat", "↓ on the last line goes to its end first");
    press(&mut c, KeyCode::Down);
    assert_eq!(focused(&mut c), "tab ≡", "↓ at the end of the last line leaves forward");
    // ← and → move the cursor; Tab completes.
    let mut c = chat();
    typed(&mut c, "#dep");
    press(&mut c, KeyCode::Left);
    press(&mut c, KeyCode::Right);
    assert_eq!(focused(&mut c), "mode chat");
    press(&mut c, KeyCode::Tab);
    assert_eq!((focused(&mut c), c.input().text()), ("mode chat".to_string(), "#deploy "), "Tab completes in the box");
    // Typing keys the shell takes outside the box are text inside it.
    let mut c = chat();
    typed(&mut c, "q m ? 1 0");
    assert_eq!(c.input().text(), "q m ? 1 0", "every plain key edits in the box");
}

/// The title of the menu open, if any.
fn menu(c: &mut Chat) -> Option<String> {
    rows(c).iter().find_map(|r| {
        let i = r.find("pick one: ")?;
        Some(r[i + "pick one: ".len()..].chars().take_while(|ch| *ch != '─').collect::<String>().trim().to_string())
    })
}

/// Walk forward from the box's ↓ exit until `want` has the focus.
fn focus_to(c: &mut Chat, want: &str) {
    // The tab bar backward from the tab shown, then the ring forward from it.
    for code in [KeyCode::BackTab, KeyCode::Tab] {
        if focused(c) != "mode chat" {
            press(c, KeyCode::Esc);
        }
        press(c, KeyCode::F(2));
        for _ in 0..40 {
            if focused(c) == want {
                return;
            }
            if focused(c) == "mode chat" {
                break;
            }
            press(c, code);
        }
    }
    panic!("never reached {want}");
}

#[test]
fn enter_opens_the_menu_a_click_opens_for_every_kind_of_element() {
    for (element, title) in [("tab ≡", "attend-chat"), ("tab #deploy", "#deploy"), ("tab #open", "#open"), ("tab ⋯all", "⋯all"), ("row #infra", "#infra"), ("mode feed", "@")] {
        let mut c = chat();
        focus_to(&mut c, element);
        press(&mut c, KeyCode::Enter);
        let m = menu(&mut c).unwrap_or_else(|| panic!("{element}: no menu: {}", screen(&mut c)));
        assert!(m.starts_with(title), "{element}: {m}");
    }
    // An agent's row: its menu.
    let mut c = chat();
    focus_to(&mut c, "mode feed");
    press(&mut c, KeyCode::BackTab);
    press(&mut c, KeyCode::BackTab);
    press(&mut c, KeyCode::BackTab);
    press(&mut c, KeyCode::BackTab);
    assert!(focused(&mut c).starts_with("row "), "{}", focused(&mut c));
    press(&mut c, KeyCode::Enter);
    assert!(menu(&mut c).is_some_and(|m| m.starts_with('@')), "{}", screen(&mut c));
    // `+`: the new channel's name, asked in its dialog.
    let mut c = chat();
    focus_to(&mut c, "tab +");
    press(&mut c, KeyCode::Enter);
    assert!(screen(&mut c).contains("new channel name"), "{}", screen(&mut c));
}

#[test]
fn the_operator_view_is_no_channel_and_the_reserved_tabs_sit_right() {
    let mut c = chat();
    let top = rows(&mut c)[0].clone();
    assert!(top.trim_end().ends_with("#open   ⋯all") || top.trim_end().ends_with("#open  ⋯all") || top.trim_end().ends_with("#open ⋯all"), "#open then ⋯all at the right end: {top:?}");
    assert!(!top.contains("merged") && !top.contains("#⋯all") && !top.contains("#all"), "{top}");
    let open = top.find("#open").unwrap();
    assert!(top.find('+').unwrap() < open && top.find("#deploy").unwrap() < top.find('+').unwrap(), "created channels, then +, left of the reserved: {top}");
    assert!(open > top.chars().count() / 2, "pinned right: {top}");
    focus_to(&mut c, "tab ⋯all");
    press(&mut c, KeyCode::Enter);
    let s = screen(&mut c);
    assert!(s.contains("Clear view"), "{s}");
    for item in ["Leave", "Seal", "Delete", "Describe", "Invite", "Clear history", "Join"] {
        assert!(!s.contains(item), "the operator view offers no channel's {item}: {s}");
    }
    // At a width that cannot hold every name, the reserved tabs and `+`
    // stay whole and the created channels give way.
    let buf = testkit::render_screen(&mut c, 44, 20);
    let top = testkit::rows(&buf)[0].clone();
    assert!(top.contains("#open") && top.contains("⋯all") && top.contains('+') && top.contains('≡'), "{top:?}");
}

#[test]
fn outside_the_box_every_single_key_the_help_names_works() {
    // `?` opens the key help.
    let mut c = chat();
    press(&mut c, KeyCode::BackTab);
    press(&mut c, KeyCode::Char('?'));
    assert!(screen(&mut c).contains("keys · ↑↓ scroll"), "{}", screen(&mut c));
    // `m` turns the mouse off.
    let mut c = chat();
    press(&mut c, KeyCode::BackTab);
    press(&mut c, KeyCode::Char('m'));
    assert!(bar(&mut c).contains("mouse off"), "{}", bar(&mut c));
    // On the tab bar a digit shows that created channel; 0 the bar's last
    // tab, the operator view.
    let mut c = chat();
    press(&mut c, KeyCode::F(2));
    press(&mut c, KeyCode::Char('1'));
    assert_eq!(c.foreground(), &Tab::Channel("deploy".into()));
    press(&mut c, KeyCode::Char('0'));
    assert_eq!(c.foreground(), &Tab::Merged);
    assert!(c.input().is_empty(), "nothing was typed");
    // PgUp scrolls the feed.
    let mut c = chat();
    for i in 0..30 {
        c.push(Signal { id: format!("x-{i}"), from: format!("claude:{ALPHA_SID}"), project: "alpha".into(), cwd: "/work/alpha".into(), reply_to: None, message: format!("filler {i}"), ts: 1_790_949_100 + i, channel: Channel::Open });
    }
    press(&mut c, KeyCode::BackTab);
    press(&mut c, KeyCode::PageUp);
    assert!(c.scroll() > 0, "PgUp pages back");
    // F3 hides the column.
    let mut c = chat();
    press(&mut c, KeyCode::BackTab);
    press(&mut c, KeyCode::F(3));
    assert!(!c.sidebar_shown(), "F3 hides the column");
    // Ctrl+N asks a new channel's name.
    let mut c = chat();
    press(&mut c, KeyCode::BackTab);
    key(&mut c, KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
    assert!(screen(&mut c).contains("new channel name"));
    // Esc goes back to the box, and any other character is typed there.
    let mut c = chat();
    press(&mut c, KeyCode::BackTab);
    press(&mut c, KeyCode::Esc);
    assert_eq!(focused(&mut c), "mode chat");
    press(&mut c, KeyCode::BackTab);
    press(&mut c, KeyCode::Char('h'));
    assert_eq!((focused(&mut c), c.input().text()), ("mode chat".to_string(), "h"));
    // `q` asks first, even with nothing unsaved: `n` stays, `y` quits.
    let mut c = chat();
    press(&mut c, KeyCode::BackTab);
    assert!(press(&mut c, KeyCode::Char('q')), "q asks before it quits");
    assert!(bar(&mut c).contains("quit?") && bar(&mut c).contains(" y quit "), "{}", bar(&mut c));
    assert!(press(&mut c, KeyCode::Char('n')) && !c.app().ended(), "n stays");
    press(&mut c, KeyCode::Char('q'));
    assert!(!press(&mut c, KeyCode::Char('y')), "y quits");
}

/// The reviewer's reproduction: ↑ on an empty box, then `1` and `x`. The
/// arrow does not leave an empty box, so both characters are typed and no
/// channel is switched (ADR-409, Guarded).
#[test]
fn an_arrow_on_an_empty_box_never_turns_the_next_keys_into_commands() {
    for arrow in [KeyCode::Up, KeyCode::Down] {
        let mut c = chat();
        press(&mut c, arrow);
        typed(&mut c, "1x");
        let f = focused(&mut c);
        assert_eq!((f, c.input().text(), c.foreground().clone()), ("mode chat".to_string(), "1x", Tab::Merged), "{arrow:?}");
        // q and m are text there too.
        typed(&mut c, "qm");
        assert_eq!(c.input().text(), "1xqm");
        assert!(!c.app().ended() && c.app().mouse_on());
    }
}

/// With a draft, ↑ twice (to the start, then out) and a digit: the digit is
/// typed into the draft, which keeps its destination.
#[test]
fn a_digit_after_leaving_a_draft_is_typed_not_a_tab_switch() {
    let mut c = chat();
    typed(&mut c, "hi");
    press(&mut c, KeyCode::Up);
    press(&mut c, KeyCode::Up);
    assert_eq!(focused(&mut c), "mode feed");
    press(&mut c, KeyCode::Char('1'));
    let f = focused(&mut c);
    assert_eq!((c.foreground().clone(), c.input().text(), f), (Tab::Merged, "1hi", "mode chat".to_string()), "typed at the draft's cursor, the destination kept");
}

/// `q` outside the box over a draft: the question, then the exit guard.
#[test]
fn q_outside_the_box_goes_through_the_exit_guard_over_a_draft() {
    let mut c = chat();
    typed(&mut c, "draft");
    press(&mut c, KeyCode::BackTab);
    press(&mut c, KeyCode::Char('q'));
    assert!(bar(&mut c).contains("quit?"), "{}", bar(&mut c));
    assert!(press(&mut c, KeyCode::Char('y')), "the draft keeps the chat open");
    assert!(c.app().guarding(), "the exit guard asks over the draft");
    press(&mut c, KeyCode::Esc);
    assert!(!c.app().guarding() && !c.app().ended());
    assert_eq!(c.input().text(), "draft");
}

/// Esc leaves an empty box, backward, as Shift+Tab does.
#[test]
fn esc_leaves_an_empty_box() {
    let mut c = chat();
    press(&mut c, KeyCode::Esc);
    assert_eq!(focused(&mut c), "mode feed");
    let mut c = chat();
    typed(&mut c, "x");
    press(&mut c, KeyCode::Esc);
    assert_eq!(focused(&mut c), "mode chat", "with text, Esc only clears the bar");
}

/// Backspace and Delete outside the box go back to it and leave the draft.
#[test]
fn backspace_outside_the_box_does_not_edit_the_draft() {
    for k in [KeyCode::Backspace, KeyCode::Delete] {
        let mut c = chat();
        typed(&mut c, "abc");
        press(&mut c, KeyCode::BackTab);
        press(&mut c, k);
        let f = focused(&mut c);
        assert_eq!((f, c.input().text()), ("mode chat".to_string(), "abc"), "{k:?}");
    }
}

/// Walking the bar previews each tab; walking off it without choosing puts
/// back the tab shown before, so the draft's destination stays. Esc on a
/// tab keeps it.
#[test]
fn walking_off_the_bar_puts_back_the_tab_shown() {
    let mut c = chat();
    typed(&mut c, "x");
    press(&mut c, KeyCode::Down);
    assert_eq!(focused(&mut c), "tab ≡");
    press(&mut c, KeyCode::Tab);
    assert_eq!(c.foreground(), &Tab::Channel("deploy".into()), "the walk previews #deploy");
    for _ in 0..6 {
        press(&mut c, KeyCode::Tab);
    }
    assert!(focused(&mut c).starts_with("row "), "{}", focused(&mut c));
    assert_eq!(c.foreground(), &Tab::Merged, "off the bar: the tab shown before");
    // Esc cancels the preview, as F2 back to the box does.
    for back in [KeyCode::Esc, KeyCode::F(2)] {
        let mut c = chat();
        press(&mut c, KeyCode::F(2));
        press(&mut c, KeyCode::Left);
        assert_eq!(c.foreground(), &Tab::Channel("open".into()));
        press(&mut c, back);
        let f = focused(&mut c);
        assert_eq!((c.foreground().clone(), f), (Tab::Merged, "mode chat".to_string()), "{back:?}");
    }
    // Enter keeps it: the menu opens, then Esc closes the menu and leaves
    // the tab chosen.
    let mut c = chat();
    press(&mut c, KeyCode::F(2));
    press(&mut c, KeyCode::Left);
    press(&mut c, KeyCode::Enter);
    press(&mut c, KeyCode::Esc);
    press(&mut c, KeyCode::Esc);
    assert_eq!(c.foreground(), &Tab::Channel("open".into()), "Enter committed it");
}

/// A digit on the bar shows its tab and takes the focus there, so Enter
/// opens the menu of the tab shown.
#[test]
fn a_digit_on_the_bar_moves_the_focus_to_its_tab() {
    let mut c = chat();
    press(&mut c, KeyCode::F(2));
    press(&mut c, KeyCode::Char('2'));
    assert_eq!((c.foreground().clone(), focused(&mut c)), (Tab::Channel("infra".into()), "tab #infra".to_string()));
    press(&mut c, KeyCode::Enter);
    assert!(screen(&mut c).contains("pick one: #infra"), "{}", screen(&mut c));
    // And the digit keeps it when the focus leaves the bar.
    press(&mut c, KeyCode::Esc);
    press(&mut c, KeyCode::Esc);
    assert_eq!(c.foreground(), &Tab::Channel("infra".into()));
}

/// At the exit guard over the entry dialog, typing goes on into the
/// dialog, never the compose draft.
#[test]
fn typing_at_the_guard_over_the_dialog_goes_back_to_the_dialog() {
    let mut c = chat();
    key(&mut c, KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
    typed(&mut c, "rel");
    key(&mut c, KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(c.app().guarding());
    typed(&mut c, "ease");
    assert!(!c.app().guarding());
    assert_eq!(c.input().text(), "", "the draft is untouched");
    assert!(screen(&mut c).contains("release"), "{}", screen(&mut c));
}

/// The compose footer keeps its verbs first at 80 and 60 columns.
#[test]
fn the_compose_footer_keeps_enter_and_tab_first() {
    let mut c = chat();
    for w in [80, 60] {
        let b = testkit::rows(&testkit::render_screen(&mut c, w, 25)).pop().unwrap();
        assert!(b.contains("Enter send"), "{w}: {b}");
    }
    let b = testkit::rows(&testkit::render_screen(&mut c, 80, 25)).pop().unwrap();
    assert!(b.contains("Enter send · Tab complete"), "{b}");
}

#[test]
fn the_help_names_the_single_keys_outside_the_box_and_the_chords_inside() {
    let mut c = chat();
    press(&mut c, KeyCode::F(1));
    let inside = screen(&mut c);
    assert!(inside.contains("Alt+0-9") || inside.contains("Ctrl+0-9"), "{inside}");
    assert!(inside.contains("S-Tab") && inside.contains("leaves the box"), "{inside}");
    press(&mut c, KeyCode::Esc);
    press(&mut c, KeyCode::BackTab);
    press(&mut c, KeyCode::F(1));
    let outside = screen(&mut c);
    for k in ["? ", "0-9", "q ^C", "m "] {
        assert!(outside.contains(k), "{k:?} in the help outside the box: {outside}");
    }
}

#[test]
fn the_footer_says_what_the_focused_element_takes() {
    let mut c = chat();
    assert!(bar(&mut c).contains("S-Tab or ↑") && bar(&mut c).contains("leaves"), "{}", bar(&mut c));
    for (element, want) in [("tab #deploy", "Enter its menu"), ("tab ⋯all", "Enter its menu"), ("tab ≡", "Enter the menu"), ("tab +", "Enter new channel"), ("row #infra", "↑ ↓ move"), ("mode feed", "Enter sender's menu")] {
        let mut c = chat();
        focus_to(&mut c, element);
        let b = bar(&mut c);
        assert!(b.contains(want) && b.contains("Tab next") && b.contains("S-Tab back"), "{element}: {b}");
        assert!(!b.contains("leaves the box"), "{element}: {b}");
    }
    let mut c = chat();
    focus_to(&mut c, "tab ⋯all");
    assert!(bar(&mut c).contains(" view "), "the lozenge names the operator view: {}", bar(&mut c));
}

#[test]
fn the_compose_border_is_subdued_unless_the_box_has_the_focus() {
    let mut c = chat();
    let style = |c: &mut Chat| {
        let buf = testkit::render_screen(c, W, H);
        let rows = testkit::rows(&buf);
        let y = rows.iter().rposition(|r| r.contains("> ")).unwrap() - 1;
        let x = rows[y].chars().position(|ch| ch == '╭').unwrap();
        buf[(x as u16, y as u16)].style()
    };
    // The feed's top-left corner: its border is the subdued rule while the
    // feed does not have the focus, the accent while it does.
    let feed = |c: &mut Chat| {
        let buf = testkit::render_screen(c, W, H);
        let rows = testkit::rows(&buf);
        let x = rows[1].chars().position(|ch| ch == '╭').unwrap();
        buf[(x as u16, 1)].style()
    };
    let in_box = style(&mut c);
    let feed_away = feed(&mut c);
    press(&mut c, KeyCode::BackTab);
    let away = style(&mut c);
    let feed_focused = feed(&mut c);
    assert_ne!(in_box, away, "the box's border changes with the focus");
    assert_eq!(away, feed_away, "away from the box: the subdued border every unfocused box has");
    assert_eq!(in_box, feed_focused, "in the box: the focused border");
}

#[test]
fn the_wheel_scrolls_what_is_under_it() {
    let mut c = chat();
    for i in 0..30 {
        c.push(Signal { id: format!("w-{i}"), from: format!("claude:{ALPHA_SID}"), project: "alpha".into(), cwd: "/work/alpha".into(), reply_to: None, message: format!("filler {i}"), ts: 1_790_949_100 + i, channel: Channel::Open });
    }
    let rows = rows(&mut c);
    let compose = rows.iter().rposition(|r| r.contains("> ")).unwrap() as u16;
    let wheel = |c: &mut Chat, x: u16, y: u16| {
        testkit::render_screen(c, W, H);
        c.mouse(MouseEvent { kind: MouseEventKind::ScrollUp, column: x, row: y, modifiers: KeyModifiers::NONE });
    };
    wheel(&mut c, 80, compose);
    assert_eq!(c.scroll(), 0, "over the compose box the feed does not move");
    wheel(&mut c, 80, 5);
    assert!(c.scroll() > 0, "over the feed it scrolls");
    // Over the key help, the help scrolls and the feed stays.
    let before = c.scroll();
    press(&mut c, KeyCode::F(1));
    wheel(&mut c, W / 2, H / 2);
    assert_eq!(c.scroll(), before, "the help is under the pointer");
}

#[test]
fn a_new_channel_is_named_in_its_dialog_and_the_draft_is_untouched() {
    let mut c = chat();
    typed(&mut c, "my draft");
    key(&mut c, KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
    let s = screen(&mut c);
    assert!(s.contains("new channel name") && s.contains("Enter sets · Esc cancels"), "{s}");
    typed(&mut c, "bad name");
    press(&mut c, KeyCode::Enter);
    let s = screen(&mut c);
    assert!(s.contains("new channel name") && s.contains("bad name"), "the dialog stays open with the name: {s}");
    assert_eq!(c.input().text(), "my draft", "the compose box was never borrowed");
    // The dialog says what is wrong inside it.
    let dialog: Vec<String> = rows(&mut c).into_iter().skip_while(|r| !r.contains("new channel name")).take(4).collect();
    assert!(dialog.iter().any(|r| r.contains("bad name")) && dialog.len() >= 3, "{dialog:?}");
    press(&mut c, KeyCode::Esc);
    assert!(!screen(&mut c).contains("new channel name"), "Esc closes it");
    assert_eq!(c.input().text(), "my draft");
    assert!(press(&mut c, KeyCode::Esc), "and never quits");
    // A good name is created (a dry run says so) and the dialog closes.
    key(&mut c, KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
    typed(&mut c, "release");
    press(&mut c, KeyCode::Enter);
    assert!(c.status().contains("/channels create not run"), "{}", c.status());
    assert_eq!(c.input().text(), "my draft");
}

#[test]
fn describe_asks_its_line_in_the_same_dialog() {
    let mut c = chat();
    focus_to(&mut c, "tab #deploy");
    press(&mut c, KeyCode::Enter);
    for _ in 0..3 {
        press(&mut c, KeyCode::Down);
    }
    assert!(screen(&mut c).contains("Describe…"));
    let s = screen(&mut c);
    let _ = s;
    // Pick Describe… by its row.
    let buf = testkit::render_screen(&mut c, W, H);
    let (x, y) = testkit::find(&buf, "Describe…").unwrap();
    c.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE });
    let s = screen(&mut c);
    assert!(s.contains("describe #deploy") && s.contains("rollout"), "the dialog opens on the line it has: {s}");
    assert!(rows(&mut c).iter().rev().nth(3).is_some_and(|r| !r.contains("describe #deploy >")), "not in the compose box");
}

/// The rows of the open menu titled `title`, inside its box.
fn picker(c: &mut Chat, title: &str) -> Vec<String> {
    let rows = rows(c);
    let head = format!("pick one: {title}");
    let y = rows.iter().position(|r| r.contains(&head)).unwrap_or_else(|| panic!("no {head}: {rows:#?}"));
    let top: Vec<char> = rows[y].chars().collect();
    let left = top.iter().position(|ch| *ch == '┌').unwrap();
    let right = top.iter().rposition(|ch| *ch == '┐').unwrap();
    rows[y + 1..].iter().take_while(|r| r.chars().nth(left) == Some('│')).map(|r| r.chars().skip(left + 1).take(right - left - 1).collect()).collect()
}

/// The menu of the agent row named `name`, by a click on it.
fn agent_menu(c: &mut Chat, name: &str) {
    let rows = rows(c);
    let (y, r) = rows.iter().enumerate().find(|(_, r)| r.split('│').next().is_some_and(|l| l.contains(name))).unwrap_or_else(|| panic!("no {name} row: {rows:#?}"));
    let x = r.find(name).unwrap();
    let x = r[..x].chars().count() as u16;
    c.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y as u16, modifiers: KeyModifiers::NONE });
}

#[test]
fn add_agent_to_chat_lists_the_channels_the_agent_is_not_in_and_invites_it() {
    let mut c = chat();
    let names = rows(&mut c);
    let beta = names.iter().find_map(|r| r.split('│').next().and_then(|l| l.split_whitespace().find(|w| w.contains("-beta") || w.starts_with("beta")).map(str::to_string)));
    let beta = beta.unwrap_or_else(|| panic!("beta's row: {names:#?}"));
    agent_menu(&mut c, &beta);
    let s = screen(&mut c);
    assert!(s.contains("Add agent to chat ▸"), "{s}");
    let buf = testkit::render_screen(&mut c, W, H);
    let (x, y) = testkit::find(&buf, "Add agent to chat").unwrap();
    c.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE });
    assert_eq!(menu(&mut c).as_deref(), Some("add to channel"));
    let s = screen(&mut c);
    let items = picker(&mut c, "add to channel");
    assert!(items.iter().any(|r| r.contains("#deploy")) && items.iter().any(|r| r.contains("#infra")), "{s}");
    assert!(!items.iter().any(|r| r.contains("#open") || r.contains("⋯all")), "the base channel and the operator view are not offered: {items:?}");
    press(&mut c, KeyCode::Enter);
    assert!(c.status().contains("/invite not run"), "the pick invites: {}", c.status());
    // alpha is in #deploy already: only #infra is offered.
    let mut c = chat();
    let alpha = rows(&mut c).iter().find_map(|r| r.split('│').next().and_then(|l| l.split_whitespace().find(|w| w.contains("-alpha") || w.starts_with("alpha")).map(str::to_string))).unwrap();
    agent_menu(&mut c, &alpha);
    let buf = testkit::render_screen(&mut c, W, H);
    let (x, y) = testkit::find(&buf, "Add agent to chat").unwrap();
    c.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE });
    let items = picker(&mut c, "add to channel");
    assert!(items.iter().any(|r| r.contains("#infra")) && !items.iter().any(|r| r.contains("#deploy")), "{items:?}");
}
