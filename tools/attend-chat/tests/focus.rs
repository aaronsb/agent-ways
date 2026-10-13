//! One focus owner and the keyboard's ring (ADR-405, #931 increments 5 and
//! 6; ADR-407's redesign), through the shell's real key and mouse handlers:
//! the ring's order, Tab and Shift+Tab as mirrors, every way out of the
//! compose box, the Esc cycle, Enter opening each element's menu and the
//! feed's message menu, the single keys the key help names working on the
//! tab bar and the column and typed in the chat, the column's numbers, the
//! footer's keys per focus, the operator view and the reserved tabs at the
//! bar's right end, the entry dialog that replaced the lent compose box,
//! and Add agent to chat. In its own file, so its own fixture: alpha and beta live,
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
    // #deploy shown, the column's second channel, so the bar holds its tab.
    show_deploy(&mut c);
    // Tab in an empty box has nothing to complete: it leaves forward.
    press(&mut c, KeyCode::Tab);
    let ring = {
        let mut r = vec![focused(&mut c)];
        r.extend(walk(&mut c, KeyCode::Tab, "mode chat"));
        r
    };
    let tabs: Vec<&str> = ring.iter().filter(|f| f.starts_with("tab ")).map(String::as_str).collect();
    assert_eq!(tabs, ["tab ≡", "tab #deploy", "tab +", "tab #open", "tab ⋯all"], "{ring:?}");
    assert_eq!(c.foreground(), &Tab::Channel("deploy".into()), "walking the bar shows nothing");
    let rows: Vec<&str> = ring.iter().filter(|f| f.starts_with("row ")).map(String::as_str).collect();
    let channels: Vec<&str> = rows.iter().copied().filter(|r| r.starts_with("row #")).collect();
    assert_eq!(channels, ["row #open", "row #deploy", "row #infra"], "every channel, in the column's numbered order: {ring:?}");
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

/// Tab and Shift+Tab are exact mirrors around the one ring: from every
/// element, N × Tab then N × Shift+Tab comes back, for every N round it.
#[test]
fn tab_and_shift_tab_are_mirrors_from_every_element() {
    let mut c = chat();
    show_deploy(&mut c);
    press(&mut c, KeyCode::Tab);
    let mut ring = vec![focused(&mut c)];
    ring.extend(walk(&mut c, KeyCode::Tab, "mode chat"));
    assert!(ring.len() >= 12, "five slots, the column, the feed and the box: {ring:?}");
    for start in &ring {
        for n in 1..=ring.len() {
            let mut c = chat();
            show_deploy(&mut c);
            focus_to(&mut c, start);
            for _ in 0..n {
                press(&mut c, KeyCode::Tab);
            }
            for _ in 0..n {
                press(&mut c, KeyCode::BackTab);
            }
            assert_eq!(&focused(&mut c), start, "{n} × Tab, then {n} × Shift+Tab");
        }
    }
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
    // ← and → move the cursor; Tab completes a word to complete.
    let mut c = chat();
    typed(&mut c, "#dep");
    press(&mut c, KeyCode::Left);
    press(&mut c, KeyCode::Right);
    assert_eq!(focused(&mut c), "mode chat");
    press(&mut c, KeyCode::Tab);
    assert_eq!((focused(&mut c), c.input().text()), ("mode chat".to_string(), "#deploy "), "Tab completes in the box");
    let mut c = chat();
    typed(&mut c, "#");
    press(&mut c, KeyCode::Tab);
    assert_eq!(c.input().text(), "#open ");
    press(&mut c, KeyCode::Tab);
    assert_eq!((focused(&mut c), c.input().text()), ("mode chat".to_string(), "#deploy "), "again: the next candidate");
    // With nothing to complete, Tab leaves forward and the draft stays.
    for draft in ["", "hello", "hello #infra "] {
        let mut c = chat();
        typed(&mut c, draft);
        press(&mut c, KeyCode::Tab);
        assert_eq!((focused(&mut c), c.input().text()), ("tab ≡".to_string(), draft), "Tab after {draft:?} leaves");
    }
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

/// Walk the ring forward with Tab until `want` has the focus.
fn focus_to(c: &mut Chat, want: &str) {
    for _ in 0..40 {
        if focused(c) == want {
            return;
        }
        press(c, KeyCode::Tab);
    }
    panic!("never reached {want}: {}", screen(c));
}

/// #deploy shown, so the tab bar holds its tab: Ctrl+2, the column's
/// second channel.
fn show_deploy(c: &mut Chat) {
    key(c, KeyEvent::new(KeyCode::Char('2'), KeyModifiers::CONTROL));
    assert_eq!(c.foreground(), &Tab::Channel("deploy".into()));
}

#[test]
fn enter_opens_the_menu_a_click_opens_for_every_kind_of_element() {
    for (element, title) in [("tab ≡", "attend-chat"), ("tab #deploy", "#deploy"), ("tab #open", "#open"), ("tab ⋯all", "⋯all"), ("row #infra", "#infra"), ("mode feed", "message from @")] {
        let mut c = chat();
        if element == "tab #deploy" {
            show_deploy(&mut c);
        }
        focus_to(&mut c, element);
        press(&mut c, KeyCode::Enter);
        let m = menu(&mut c).unwrap_or_else(|| panic!("{element}: no menu: {}", screen(&mut c)));
        assert!(m.starts_with(title), "{element}: {m}");
    }
    // Enter on #open's tab opens its menu and shows nothing.
    let mut c = chat();
    show_deploy(&mut c);
    focus_to(&mut c, "tab #open");
    press(&mut c, KeyCode::Enter);
    assert_eq!(c.foreground(), &Tab::Channel("deploy".into()), "the menu, not a switch");
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
    assert!(!top.contains("#deploy") && !top.contains("#infra"), "no tab per channel: {top}");
    let open = top.find("#open").unwrap();
    assert!(top.find('≡').unwrap() < top.find('+').unwrap() && top.find('+').unwrap() < open, "≡ and + left of the reserved: {top}");
    assert!(open > top.chars().count() / 2, "pinned right: {top}");
    // A created channel shown takes the place between ≡ and +, with what
    // it is for, unnumbered: the column numbers the channels.
    show_deploy(&mut c);
    let top = rows(&mut c)[0].clone();
    let deploy = top.find("#deploy — rollout").unwrap_or_else(|| panic!("the channel shown and its line: {top}"));
    assert!(top.find('≡').unwrap() < deploy && deploy < top.find('+').unwrap(), "{top}");
    assert!(!top.contains("1 #deploy") && !top.contains("2 #deploy"), "no number on the bar: {top}");
    focus_to(&mut c, "tab ⋯all");
    press(&mut c, KeyCode::Enter);
    let s = screen(&mut c);
    assert!(s.contains("Clear view"), "{s}");
    for item in ["Leave", "Seal", "Delete", "Describe", "Invite", "Clear history", "Join"] {
        assert!(!s.contains(item), "the operator view offers no channel's {item}: {s}");
    }
    // At a width that cannot hold the channel's line, the reserved tabs and
    // `+` stay whole and the channel's tab gives way.
    let buf = testkit::render_screen(&mut c, 44, 20);
    let top = testkit::rows(&buf)[0].clone();
    assert!(top.contains("#open") && top.contains("⋯all") && top.contains('+') && top.contains('≡') && top.contains("#d"), "{top:?}");
    assert!(!top.contains("rollout"), "the line is cut first: {top:?}");
}

/// On the tab bar and the column the shell's single keys work as the key
/// help names them; in the compose box and the feed, the chat, they are
/// typed.
#[test]
fn outside_the_chat_every_single_key_the_help_names_works() {
    // The column's last row: Shift+Tab twice from the box.
    let to_column = |c: &mut Chat| {
        press(c, KeyCode::BackTab);
        press(c, KeyCode::BackTab);
        assert!(focused(c).starts_with("row "), "{}", focused(c));
    };
    // `?` opens the key help.
    let mut c = chat();
    to_column(&mut c);
    press(&mut c, KeyCode::Char('?'));
    assert!(screen(&mut c).contains("keys · ↑↓ scroll"), "{}", screen(&mut c));
    // `m` turns the mouse off.
    let mut c = chat();
    to_column(&mut c);
    press(&mut c, KeyCode::Char('m'));
    assert!(bar(&mut c).contains("mouse off"), "{}", bar(&mut c));
    // On a column row a digit shows the channel by its number, 0 the
    // operator view.
    let mut c = chat();
    to_column(&mut c);
    press(&mut c, KeyCode::Char('2'));
    assert_eq!(c.foreground(), &Tab::Channel("deploy".into()));
    assert_eq!(focused(&mut c), "row #deploy", "the cursor moves to the row picked");
    press(&mut c, KeyCode::Char('0'));
    assert_eq!(c.foreground(), &Tab::Merged);
    assert!(c.input().is_empty(), "nothing was typed");
    // PgUp scrolls the feed.
    let mut c = chat();
    for i in 0..30 {
        c.push(Signal { id: format!("x-{i}"), from: format!("claude:{ALPHA_SID}"), project: "alpha".into(), cwd: "/work/alpha".into(), reply_to: None, message: format!("filler {i}"), ts: 1_790_949_100 + i, channel: Channel::Open });
    }
    to_column(&mut c);
    press(&mut c, KeyCode::PageUp);
    assert!(c.scroll() > 0, "PgUp pages back");
    // F3 hides the column.
    let mut c = chat();
    to_column(&mut c);
    press(&mut c, KeyCode::F(3));
    assert!(!c.sidebar_shown(), "F3 hides the column");
    // Ctrl+N asks a new channel's name.
    let mut c = chat();
    to_column(&mut c);
    key(&mut c, KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
    assert!(screen(&mut c).contains("new channel name"));
    // Any other character is typed in the box.
    let mut c = chat();
    to_column(&mut c);
    press(&mut c, KeyCode::Char('h'));
    assert_eq!((focused(&mut c), c.input().text()), ("mode chat".to_string(), "h"));
    // `q` asks first, even with nothing unsaved: `n` stays, `y` quits.
    let mut c = chat();
    to_column(&mut c);
    assert!(press(&mut c, KeyCode::Char('q')), "q asks before it quits");
    assert!(bar(&mut c).contains("quit?") && bar(&mut c).contains(" y quit "), "{}", bar(&mut c));
    assert!(press(&mut c, KeyCode::Char('n')) && !c.app().ended(), "n stays");
    press(&mut c, KeyCode::Char('q'));
    assert!(!press(&mut c, KeyCode::Char('y')), "y quits");
}

/// The feed is the chat: a printable key there goes to the compose box,
/// typed, and `q`, `m`, `?` and the digits are text, never commands.
#[test]
fn a_key_typed_in_the_feed_goes_to_the_box() {
    for (k, want) in [('h', "h"), ('q', "q"), ('m', "m"), ('?', "?"), ('2', "2")] {
        let mut c = chat();
        press(&mut c, KeyCode::BackTab);
        assert_eq!(focused(&mut c), "mode feed");
        press(&mut c, KeyCode::Char(k));
        let f = focused(&mut c);
        assert_eq!((f, c.input().text(), c.foreground().clone()), ("mode chat".to_string(), want, Tab::Merged), "{k:?} in the feed");
        assert!(!bar(&mut c).contains("quit?") && c.app().mouse_on() && !screen(&mut c).contains("keys · ↑↓ scroll"), "{k:?} is no command there");
    }
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
    press(&mut c, KeyCode::BackTab);
    press(&mut c, KeyCode::Char('q'));
    assert!(bar(&mut c).contains("quit?"), "{}", bar(&mut c));
    assert!(press(&mut c, KeyCode::Char('y')), "the draft keeps the chat open");
    assert!(c.app().guarding(), "the exit guard asks over the draft");
    press(&mut c, KeyCode::Esc);
    assert!(!c.app().guarding() && !c.app().ended());
    assert_eq!(c.input().text(), "draft");
}

/// The Esc cycle, through the real keys: the box, the feed, the `≡` menu,
/// the tab or column row last focused (the feed when there is none), the
/// `≡` menu again. The draft stays through it all.
#[test]
fn esc_moves_along_one_cycle() {
    let common = |c: &mut Chat| menu(c).is_some_and(|m| m.starts_with("attend-chat"));
    for draft in ["", "x"] {
        let mut c = chat();
        typed(&mut c, draft);
        press(&mut c, KeyCode::Esc);
        assert_eq!(focused(&mut c), "mode feed", "{draft:?}: the box, then the feed");
        press(&mut c, KeyCode::Esc);
        assert!(common(&mut c), "{draft:?}: then the ≡ menu: {}", screen(&mut c));
        assert_eq!(focused(&mut c), "tab ≡", "the focus on ≡ while it is open");
        press(&mut c, KeyCode::Esc);
        assert!(menu(&mut c).is_none());
        assert_eq!(focused(&mut c), "mode feed", "{draft:?}: no tab or row was focused: back to the feed");
        press(&mut c, KeyCode::Esc);
        assert!(common(&mut c), "{draft:?}: the ≡ menu again");
        press(&mut c, KeyCode::Esc);
        assert_eq!(c.input().text(), draft, "the draft stays");
    }
    // From a column row: the menu, then back to the row, and round.
    let mut c = chat();
    press(&mut c, KeyCode::BackTab);
    press(&mut c, KeyCode::BackTab);
    assert_eq!(focused(&mut c), "row #infra");
    for _ in 0..2 {
        press(&mut c, KeyCode::Esc);
        assert!(common(&mut c), "{}", screen(&mut c));
        press(&mut c, KeyCode::Esc);
        assert_eq!((menu(&mut c), focused(&mut c)), (None, "row #infra".to_string()), "back to the row last focused");
    }
    // From a tab: back to it.
    let mut c = chat();
    focus_to(&mut c, "tab #open");
    press(&mut c, KeyCode::Esc);
    assert!(common(&mut c));
    press(&mut c, KeyCode::Esc);
    assert_eq!(focused(&mut c), "tab #open");
    // The feed remembers the row it was reached from.
    let mut c = chat();
    press(&mut c, KeyCode::BackTab);
    press(&mut c, KeyCode::BackTab);
    press(&mut c, KeyCode::Tab);
    assert_eq!(focused(&mut c), "mode feed");
    press(&mut c, KeyCode::Esc);
    press(&mut c, KeyCode::Esc);
    assert_eq!(focused(&mut c), "row #infra", "≡, then the last row focused");
    // An overlay that is open takes the first Esc.
    let mut c = chat();
    typed(&mut c, "draft");
    press(&mut c, KeyCode::F(1));
    press(&mut c, KeyCode::Esc);
    assert_eq!(focused(&mut c), "mode chat", "the key help closed, the box keeps the focus");
    let mut c = chat();
    focus_to(&mut c, "tab ≡");
    press(&mut c, KeyCode::Enter);
    assert!(common(&mut c));
    press(&mut c, KeyCode::Esc);
    assert_eq!((menu(&mut c), focused(&mut c)), (None, "tab ≡".to_string()), "a menu Enter opened closes where it was");
    assert!(press(&mut c, KeyCode::Esc), "and Esc never quits");
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

/// Walking the tab bar shows nothing: the tab shown stays, and the draft's
/// destination with it. Enter on a tab opens its menu.
#[test]
fn walking_the_bar_shows_nothing() {
    let mut c = chat();
    typed(&mut c, "x");
    press(&mut c, KeyCode::Down);
    assert_eq!(focused(&mut c), "tab ≡");
    for want in ["tab +", "tab #open", "tab ⋯all"] {
        press(&mut c, KeyCode::Tab);
        let f = focused(&mut c);
        assert_eq!((f, c.foreground().clone()), (want.to_string(), Tab::Merged));
    }
    press(&mut c, KeyCode::F(2));
    assert_eq!(focused(&mut c), "mode chat");
    press(&mut c, KeyCode::F(2));
    assert_eq!(focused(&mut c), "tab ⋯all", "F2: the tab shown");
    press(&mut c, KeyCode::Left);
    assert_eq!((focused(&mut c), c.foreground().clone()), ("tab #open".to_string(), Tab::Merged));
    press(&mut c, KeyCode::Enter);
    assert_eq!(menu(&mut c).as_deref(), Some("#open"), "Enter: its menu");
    assert_eq!(c.foreground(), &Tab::Merged, "and nothing shown");
}

/// A plain digit is the column's number only on a column row; on the tab
/// bar it is typed, as anywhere else.
#[test]
fn a_digit_picks_a_channel_on_a_column_row_and_is_typed_elsewhere() {
    let mut c = chat();
    press(&mut c, KeyCode::F(2));
    press(&mut c, KeyCode::Char('2'));
    assert_eq!((c.foreground().clone(), focused(&mut c), c.input().text()), (Tab::Merged, "mode chat".to_string(), "2"), "typed from the bar");
    let mut c = chat();
    press(&mut c, KeyCode::BackTab);
    press(&mut c, KeyCode::BackTab);
    press(&mut c, KeyCode::Char('3'));
    assert_eq!((c.foreground().clone(), focused(&mut c)), (Tab::Channel("infra".into()), "row #infra".to_string()));
    press(&mut c, KeyCode::Char('3'));
    assert!(menu(&mut c).is_none(), "a plain digit on the channel shown opens no menu");
    press(&mut c, KeyCode::Enter);
    assert_eq!(menu(&mut c).as_deref(), Some("#infra"), "Enter does");
    press(&mut c, KeyCode::Esc);
    press(&mut c, KeyCode::Char('9'));
    assert_eq!(c.foreground(), &Tab::Channel("infra".into()), "a number past the channels does nothing");
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
fn the_help_names_the_single_keys_outside_the_chat_and_the_chords_inside() {
    let mut c = chat();
    press(&mut c, KeyCode::F(1));
    let inside = screen(&mut c);
    assert!(inside.contains("Ctrl+0-9"), "{inside}");
    assert!(inside.contains("S-Tab") && inside.contains("leaves the box"), "{inside}");
    press(&mut c, KeyCode::Esc);
    press(&mut c, KeyCode::BackTab);
    press(&mut c, KeyCode::BackTab);
    press(&mut c, KeyCode::F(1));
    let outside = screen(&mut c);
    for k in ["? ", "0-9 channels", "q ^C", "m "] {
        assert!(outside.contains(k), "{k:?} in the help on a column row: {outside}");
    }
}

#[test]
fn the_footer_says_what_the_focused_element_takes() {
    let mut c = chat();
    assert!(bar(&mut c).contains("S-Tab or ↑") && bar(&mut c).contains("leaves"), "{}", bar(&mut c));
    for (element, want) in [("tab #deploy", "Enter its menu"), ("tab ⋯all", "Enter its menu"), ("tab ≡", "Enter the menu"), ("tab +", "Enter new channel"), ("row #infra", "↑ ↓ move"), ("mode feed", "Enter message menu")] {
        let mut c = chat();
        if element == "tab #deploy" {
            show_deploy(&mut c);
        }
        focus_to(&mut c, element);
        let b = bar(&mut c);
        assert!(b.contains(want) && b.contains("Tab next") && b.contains("S-Tab back") && b.contains("Esc ≡ menu"), "{element}: {b}");
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
    show_deploy(&mut c);
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

/// A left click on the first `text` left of the column's rule, below the
/// tab bar: a row of the column.
fn side_click(c: &mut Chat, text: &str) {
    let rows = rows(c);
    let (x, y) = rows
        .iter()
        .enumerate()
        .skip(1)
        .find_map(|(y, r)| {
            let col = r.split('│').next()?;
            col.find(text).map(|b| (col[..b].chars().count() as u16, y as u16))
        })
        .unwrap_or_else(|| panic!("no {text} in the column"));
    c.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE });
}

/// The open menu's box as drawn: where it sits, and its lines.
fn menu_box(c: &mut Chat) -> Option<(usize, usize, Vec<String>)> {
    let rows: Vec<Vec<char>> = rows(c).iter().map(|r| r.chars().collect()).collect();
    let (y, x) = rows.iter().enumerate().find_map(|(y, r)| {
        let s: String = r.iter().collect();
        s.find("┌pick one").map(|b| (y, s[..b].chars().count()))
    })?;
    let w = rows[y][x..].iter().position(|ch| *ch == '┐')? + 1;
    let mut out = Vec::new();
    for r in &rows[y..] {
        let line: String = r[x..x + w].iter().collect();
        let end = line.starts_with('└');
        out.push(line);
        if end {
            break;
        }
    }
    Some((x, y, out))
}

#[test]
fn a_click_on_a_row_gives_it_the_keyboard_and_the_arrows_move_from_it() {
    let mut c = chat();
    side_click(&mut c, "#open");
    assert_eq!(focused(&mut c), "row #open", "the clicked row has the keyboard");
    assert!(bar(&mut c).trim_start().starts_with("column"), "{}", bar(&mut c));
    let mut seen = Vec::new();
    for code in [KeyCode::Down, KeyCode::Down, KeyCode::Up] {
        press(&mut c, code);
        seen.push(focused(&mut c));
    }
    assert_eq!(seen, ["row #deploy", "row #infra", "row #deploy"], "↓↓↑ along the column as drawn");
}

#[test]
fn enter_on_a_row_or_on_the_common_slot_opens_the_menu_a_click_opens_where_it_opens() {
    // A channel's row: the click that opens its menu is the second, on the
    // channel the first showed; Enter on the row the first click focused.
    let mut clicked = chat();
    side_click(&mut clicked, "#infra");
    side_click(&mut clicked, "#infra");
    let mut keyed = chat();
    side_click(&mut keyed, "#infra");
    press(&mut keyed, KeyCode::Enter);
    let want = menu_box(&mut clicked).expect("the click opens #infra's menu");
    assert!(want.2[0].contains("#infra"), "{want:?}");
    assert_eq!(menu_box(&mut keyed), Some(want), "Enter: the same menu, docked in the same place");
    // An agent's row: one click opens its menu. Esc closes it, the row
    // keeping the keyboard, and Enter opens it again where it was.
    let mut c = chat();
    side_click(&mut c, "beta");
    let want = menu_box(&mut c).expect("the click opens the agent's menu");
    assert!(want.2[0].contains('@'), "{want:?}");
    press(&mut c, KeyCode::Esc);
    assert_eq!(menu_box(&mut c), None);
    assert!(focused(&mut c).starts_with("row "), "{}", focused(&mut c));
    press(&mut c, KeyCode::Enter);
    assert_eq!(menu_box(&mut c), Some(want), "Enter: the same menu, docked in the same place");
    // ≡, reached as the operator reaches it: Shift+Tab from the box, with
    // and without a draft.
    let mut clicked = chat();
    let buf = testkit::render_screen(&mut clicked, W, H);
    let (x, y) = testkit::find(&buf, "≡").unwrap();
    clicked.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE });
    let want = menu_box(&mut clicked).expect("a click on ≡ opens its menu");
    for draft in ["", "x"] {
        let mut keyed = chat();
        typed(&mut keyed, draft);
        for _ in 0..40 {
            if focused(&mut keyed) == "tab ≡" {
                break;
            }
            press(&mut keyed, KeyCode::BackTab);
        }
        assert_eq!(focused(&mut keyed), "tab ≡");
        press(&mut keyed, KeyCode::Enter);
        assert_eq!(menu_box(&mut keyed), Some(want.clone()), "draft {draft:?}: Enter on ≡ opens the menu a click opens");
    }
}

/// Enter on the feed opens the selected message's menu, docked under its
/// header: Reply, Quote, Address, Copy. ↑ and ↓ choose the message.
#[test]
fn the_feeds_message_menu_replies_quotes_addresses_and_copies() {
    let pick = |c: &mut Chat, item: &str| {
        let buf = testkit::render_screen(c, W, H);
        let (x, y) = testkit::find(&buf, item).unwrap_or_else(|| panic!("no {item}: {}", testkit::rows(&buf).join("\n")));
        c.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE });
    };
    let open = |c: &mut Chat, ups: usize| {
        press(c, KeyCode::BackTab);
        for _ in 0..ups {
            press(c, KeyCode::Up);
        }
        press(c, KeyCode::Enter);
    };
    let mut c = chat();
    open(&mut c, 1);
    let title = menu(&mut c).unwrap_or_else(|| panic!("{}", screen(&mut c)));
    assert!(title.starts_with("message from @"), "{title}");
    let items = picker(&mut c, &title);
    let labels: Vec<String> = items.iter().map(|r| r.trim().to_string()).collect();
    assert_eq!(labels.len(), 4, "{labels:?}");
    assert!(labels[0] == "Reply" && labels[1] == "Quote" && labels[2].starts_with("Address @") && labels[3] == "Copy", "{labels:?}");
    // Quote: the newest message's lines as `> ` lines, the keyboard in the box.
    pick(&mut c, "Quote");
    assert_eq!((focused(&mut c), c.input().text()), ("mode chat".to_string(), "> message 1 in the feed\n"));
    // Copy: the older message, chosen with ↑ twice, to the clipboard.
    let mut c = chat();
    open(&mut c, 2);
    pick(&mut c, "Copy");
    assert_eq!(c.app().copied(), Some("message 0 in the feed"));
    assert!(c.status().contains("clipboard"), "{}", c.status());
    // Address: the sender's name in the box.
    let mut c = chat();
    open(&mut c, 1);
    let name = labels[2].trim_start_matches("Address ").to_string();
    pick(&mut c, "Address @");
    assert_eq!((focused(&mut c), c.input().text().to_string()), ("mode chat".to_string(), format!("{name} ")));
    // Reply: the message's channel shown, the keyboard in the box.
    let mut c = chat();
    open(&mut c, 1);
    pick(&mut c, "Reply");
    assert_eq!((focused(&mut c), c.foreground().clone()), ("mode chat".to_string(), Tab::Channel("open".into())));
}
