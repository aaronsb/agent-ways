//! An agent's DM view (ADR-405, amendment of 2026-10-13): a click on an
//! agent's row opens the direct messages between the operator and that
//! agent, listed under the agent's row and shown as a tab; the compose box
//! there sends a direct message to the agent; the view hides and shows
//! again with its messages kept. The agent's own view, Review DMs, lists
//! every direct message the agent sent or received, with anyone, and offers
//! nothing to send. Through the shell's real key and mouse handlers. In its
//! own file, so its own fixture: alpha and beta live.

mod support;

use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;
use attend_chat::signal::{self, Channel, Signal};
use attend_chat::tabs::Tab;

const ALPHA_SID: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const BETA_SID: &str = "bbbbbbbb-0000-4000-8000-000000000002";
const ALPHA_CWD: &str = "/work/alpha";
const BETA_CWD: &str = "/work/beta";
const W: u16 = 120;
const H: u16 = 35;

fn fixture() {
    support::fixture("dm-view", |_| {
        support::pin_alive(ALPHA_SID);
        support::pin_alive(BETA_SID);
    });
}

fn sig(id: &str, from: &str, cwd: &str, message: &str, channel: Channel) -> Signal {
    Signal {
        id: id.into(),
        from: from.into(),
        project: cwd.rsplit('/').next().unwrap().into(),
        cwd: cwd.into(),
        reply_to: None,
        message: message.into(),
        ts: 1_790_949_000,
        channel,
        tray: String::new(),
        beside_post: false,
    }
}

/// A chat whose feed holds DMs with alpha both ways, a DM with beta both
/// ways, DMs between alpha and beta both ways, and posts on #open and
/// #deploy. `send` lets Enter write, and feeds
/// the chat from a watcher of the fixture's trays, as `main` does, so what
/// is sent comes back.
fn chat_with(send: bool) -> Chat {
    fixture();
    let rx = send.then(|| {
        let (tx, rx) = std::sync::mpsc::channel::<attend_chat::watcher::Delivery>();
        let here = std::env::current_dir().unwrap().to_string_lossy().to_string();
        attend_chat::watcher::spawn_watcher(signal::signals_base(), signal::encode_cwd(&here), signal::human_member_id(), tx).unwrap();
        rx
    });
    let mut c = Chat::new(rx, Palette::default(), Shape::PLAIN).sidebar(true).heartbeat(false).enhanced(true).dry_run(!send).hidden_file(own_hidden());
    let alpha = format!("claude:{ALPHA_SID}");
    let beta = format!("claude:{BETA_SID}");
    let me = "external:aaron@pts-1";
    c.push(sig("s-1", &alpha, ALPHA_CWD, "alpha on open", Channel::Open));
    c.push(sig("s-2", &beta, BETA_CWD, "beta on open", Channel::Open));
    c.push(sig("s-3", &alpha, ALPHA_CWD, "alpha to you", Channel::Person));
    c.push(sig("s-4", &beta, BETA_CWD, "beta to you", Channel::Person));
    c.push(sig("s-5", me, "/work/me", "you to alpha", Channel::Sent(ALPHA_SID.into())));
    c.push(sig("s-6", me, "/work/me", "you to beta", Channel::Sent(BETA_SID.into())));
    c.push(sig("s-7", &alpha, ALPHA_CWD, "alpha in deploy", Channel::Group("deploy".into())));
    c.push(sig("s-8", &alpha, ALPHA_CWD, "alpha to beta", Channel::Elsewhere(BETA_SID.into())));
    c.push(sig("s-9", &beta, BETA_CWD, "beta to alpha", Channel::Elsewhere(ALPHA_SID.into())));
    c
}

fn chat() -> Chat {
    chat_with(false)
}

/// A state file of the chat's own for Hide messages here, so no test's
/// hide reaches another's feed.
fn own_hidden() -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let root = support::fixture("dm-view", |_| unreachable!("set up by fixture()"));
    root.join(format!("hidden-{}", NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)))
}

fn rows(c: &mut Chat) -> Vec<String> {
    testkit::rows(&testkit::render_screen(c, W, H))
}

fn screen(c: &mut Chat) -> String {
    rows(c).join("\n")
}

/// The left column's part of each row, left of its rule.
fn column(c: &mut Chat) -> Vec<String> {
    rows(c).iter().skip(1).map(|r| r.split('│').next().unwrap_or("").to_string()).collect()
}

/// The feed and the compose box: each row right of the column's rule.
fn feed(c: &mut Chat) -> String {
    rows(c).iter().skip(1).map(|r| r.split_once('│').map_or(r.as_str(), |(_, rest)| rest).to_string()).collect::<Vec<_>>().join("\n")
}

fn press(c: &mut Chat, code: KeyCode) -> bool {
    testkit::render_screen(c, W, H);
    testkit::drive(c, &[KeyEvent::new(code, KeyModifiers::NONE)])
}

fn typed(c: &mut Chat, s: &str) {
    for ch in s.chars() {
        press(c, KeyCode::Char(ch));
    }
}

/// A left click on the first `text` in the left column, below the tab bar.
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
        .unwrap_or_else(|| panic!("no {text} in the column: {rows:#?}"));
    c.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE });
}

/// The title of the menu open, if any.
fn menu(c: &mut Chat) -> Option<String> {
    rows(c).iter().find_map(|r| {
        let i = r.find("pick one: ")?;
        Some(r[i + "pick one: ".len()..].chars().take_while(|ch| *ch != '─').collect::<String>().trim().to_string())
    })
}

/// The items of the open menu, inside its box.
fn items(c: &mut Chat) -> Vec<String> {
    let rows = rows(c);
    let Some(y) = rows.iter().position(|r| r.contains("pick one: ")) else { return Vec::new() };
    let top: Vec<char> = rows[y].chars().collect();
    let left = top.iter().position(|ch| *ch == '┌').unwrap();
    let right = top.iter().rposition(|ch| *ch == '┐').unwrap();
    rows[y + 1..]
        .iter()
        .take_while(|r| r.chars().nth(left) == Some('│'))
        .map(|r| r.chars().skip(left + 1).take(right - left - 1).collect::<String>().trim().to_string())
        .collect()
}

/// Pick the open menu's item that starts with `label`, by a click.
fn pick(c: &mut Chat, label: &str) {
    let buf = testkit::render_screen(c, W, H);
    let (x, y) = testkit::find(&buf, label).unwrap_or_else(|| panic!("no {label}: {}", testkit::rows(&buf).join("\n")));
    c.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE });
}

fn nick(cwd: &str) -> String {
    agent_identity::Identity::for_cwd(cwd, agent_theme::ColorDepth::NoColor).nickname.to_string()
}

/// The column's row index holding `text`.
fn row_index(c: &mut Chat, text: &str) -> usize {
    let col = column(c);
    col.iter().position(|r| r.contains(text)).unwrap_or_else(|| panic!("no {text} in the column: {col:#?}"))
}

#[test]
fn a_click_on_an_agent_row_opens_its_dms_listed_under_it() {
    let mut c = chat();
    assert!(!column(&mut c).iter().any(|r| r.contains("DMs")), "no DM view before one is opened");
    side_click(&mut c, "alpha");
    let s = screen(&mut c);
    assert_eq!(c.foreground(), &Tab::Dm(ALPHA_SID.into()), "the click shows alpha's DMs: {s}");
    assert_eq!(menu(&mut c), None, "and opens no menu");
    let agent = row_index(&mut c, "alpha");
    let col = column(&mut c);
    assert!(col[agent + 1].contains("DMs"), "listed on the row under alpha's: {col:#?}");
    let tabs = &rows(&mut c)[0];
    assert!(tabs.contains(&format!("@{} DMs", nick(ALPHA_CWD))), "the tab bar shows the view: {tabs}");
    // A click on the agent whose DMs are shown opens its menu, as a click on
    // the channel shown does.
    side_click(&mut c, "alpha");
    assert!(menu(&mut c).is_some_and(|m| m.starts_with('@')), "{}", screen(&mut c));
}

#[test]
fn enter_on_an_agent_row_offers_its_dms_first() {
    // The keyboard on a channel's row, then up the column to alpha's.
    let mut c = chat();
    side_click(&mut c, "#open");
    while !column(&mut c).iter().any(|r| r.starts_with('▸') && r.contains("alpha")) {
        press(&mut c, KeyCode::Up);
    }
    press(&mut c, KeyCode::Enter);
    let first = items(&mut c).into_iter().next().unwrap_or_default();
    assert_eq!(first, "Show DMs", "{}", screen(&mut c));
    press(&mut c, KeyCode::Enter);
    assert_eq!(c.foreground(), &Tab::Dm(ALPHA_SID.into()));
}

#[test]
fn the_dm_view_lists_only_the_dms_with_its_agent() {
    let mut c = chat();
    side_click(&mut c, "alpha");
    let f = feed(&mut c);
    for want in ["alpha to you", "you to alpha"] {
        assert!(f.contains(want), "{want}: {f}");
    }
    for not in ["beta to you", "you to beta", "alpha on open", "beta on open", "alpha in deploy", "alpha to beta", "beta to alpha"] {
        assert!(!f.contains(not), "{not} is no DM with alpha: {f}");
    }
}

#[test]
fn compose_in_the_dm_view_sends_a_direct_message_to_the_agent() {
    let mut c = chat_with(true);
    side_click(&mut c, "alpha");
    press(&mut c, KeyCode::Char('h'));
    let flag = rows(&mut c);
    let nick = nick(ALPHA_CWD);
    assert!(flag.iter().any(|r| r.contains(&format!(" @{nick} "))), "the flag names the agent: {flag:#?}");
    typed(&mut c, "ello from the dm view");
    press(&mut c, KeyCode::Enter);
    assert!(c.input().is_empty(), "sent: {}", c.status());
    let body = "hello from the dm view";
    let tray = signal::cwd_dir(ALPHA_CWD);
    let raw = std::fs::read_dir(&tray)
        .unwrap_or_else(|e| panic!("{e}: {}", c.status()))
        .flatten()
        .map(|e| std::fs::read_to_string(e.path()).unwrap())
        .find(|r| r.contains(body))
        .expect("in alpha's tray");
    let wire = agent_identity::parse_signal(agent_identity::wire_line(&raw)).unwrap();
    assert_eq!((wire.to, wire.from_kind), (Some(ALPHA_SID), Some("human")), "{raw}");
    let open = std::fs::read_dir(signal::broadcast_dir()).into_iter().flatten().flatten().any(|e| std::fs::read_to_string(e.path()).is_ok_and(|r| r.contains(body)));
    assert!(!open, "nothing on #open");
    // Read back from alpha's tray, another project's, by the watcher.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline && !c.signals().iter().any(|s| s.message == body) {
        c.drain();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(feed(&mut c).contains(body), "the view shows what was sent: {}", screen(&mut c));
    assert_eq!(c.foreground(), &Tab::Dm(ALPHA_SID.into()));
}

#[test]
fn hiding_the_view_keeps_its_messages_and_showing_it_again_lists_them() {
    let mut c = chat();
    let before = c.signals().len();
    side_click(&mut c, "alpha");
    side_click(&mut c, "DMs");
    assert!(menu(&mut c).is_some(), "a click on the DMs row shown opens its menu: {}", screen(&mut c));
    assert!(items(&mut c).iter().any(|i| i == "Hide DMs"), "{:?}", items(&mut c));
    pick(&mut c, "Hide DMs");
    assert!(!column(&mut c).iter().any(|r| r.contains("DMs")), "the row is gone: {}", screen(&mut c));
    assert!(column(&mut c).iter().any(|r| r.starts_with('▸') && r.contains("alpha")), "the keyboard on alpha's row: {}", screen(&mut c));
    assert_eq!(c.foreground(), &Tab::Merged, "the operator view, where the DMs also show");
    assert_eq!(c.signals().len(), before, "nothing deleted");
    assert!(feed(&mut c).contains("alpha to you"), "the operator view still lists the DM");
    side_click(&mut c, "alpha");
    assert_eq!(c.foreground(), &Tab::Dm(ALPHA_SID.into()));
    let f = feed(&mut c);
    assert!(f.contains("alpha to you") && f.contains("you to alpha"), "{f}");
}

/// What has the focus: the tab behind the bar's `▸`, the column row behind
/// its `▸`, else the bottom bar's lozenge.
fn focused(c: &mut Chat) -> String {
    let rows = rows(c);
    if let Some(i) = rows[0].find('▸') {
        let first = rows[0][i + '▸'.len_utf8()..].split_whitespace().next().unwrap_or("").to_string();
        return format!("tab {first}");
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

fn walk(c: &mut Chat) -> Vec<String> {
    press(c, KeyCode::Tab);
    let mut out = vec![focused(c)];
    for _ in 0..60 {
        press(c, KeyCode::Tab);
        let f = focused(c);
        out.push(f.clone());
        if f == "mode chat" {
            return out;
        }
    }
    panic!("never back to the box: {out:?}");
}

fn focus_to(c: &mut Chat, want: &str) {
    for _ in 0..60 {
        if focused(c) == want {
            return;
        }
        press(c, KeyCode::Tab);
    }
    panic!("never reached {want}: {}", screen(c));
}

/// alpha's DMs open and shown, the keyboard back in the box.
fn dm_open() -> Chat {
    let mut c = chat();
    side_click(&mut c, "alpha");
    c.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: W - 5, row: H - 4, modifiers: KeyModifiers::NONE });
    assert_eq!(focused(&mut c), "mode chat", "{}", screen(&mut c));
    c
}

#[test]
fn tab_and_shift_tab_stay_mirrors_and_the_esc_cycle_comes_back_to_the_dm_row() {
    let ring = walk(&mut dm_open());
    assert!(ring.iter().any(|f| f == "row DMs"), "the DM row is a stop of the ring: {ring:?}");
    let nick = nick(ALPHA_CWD);
    assert!(ring.iter().any(|f| *f == format!("tab @{nick}")), "so is its tab: {ring:?}");
    let agent = ring.iter().position(|f| f.starts_with("row ") && !f.starts_with("row DMs") && !f.starts_with("row #") && ring.get(ring.iter().position(|g| g == "row DMs").unwrap() - 1) == Some(f));
    assert!(agent.is_some(), "the DM row follows its agent's row: {ring:?}");
    for start in &ring {
        for n in 1..=ring.len() {
            let mut c = dm_open();
            focus_to(&mut c, start);
            for _ in 0..n {
                press(&mut c, KeyCode::Tab);
            }
            for _ in 0..n {
                press(&mut c, KeyCode::BackTab);
            }
            assert_eq!(&focused(&mut c), start, "{n} × Tab, then {n} × Shift+Tab: {ring:?}");
        }
    }
    let mut c = dm_open();
    focus_to(&mut c, "row DMs");
    for _ in 0..2 {
        press(&mut c, KeyCode::Esc);
        assert!(menu(&mut c).is_some_and(|m| m.starts_with("attend-chat")), "the ≡ menu: {}", screen(&mut c));
        press(&mut c, KeyCode::Esc);
        assert_eq!((menu(&mut c), focused(&mut c)), (None, "row DMs".to_string()), "back to the DM row");
    }
}

/// A DM the operator sent is labelled with whom it went to, as one the
/// operator received is labelled `→ you`: on its header, not its body.
#[test]
fn a_sent_dm_is_labelled_with_its_recipient() {
    let mut c = chat();
    side_click(&mut c, "alpha");
    let want = format!("→ @{}", nick(ALPHA_CWD));
    let rows = rows(&mut c);
    let header = rows.iter().find(|r| r.contains(&want)).unwrap_or_else(|| panic!("no {want}: {rows:#?}"));
    assert!(!header.contains("you to alpha"), "the label is on the header: {header}");
    assert!(rows.iter().any(|r| r.contains("→ you")), "the received one keeps its label");
}

/// The shown view keeps its agent's name when the agent leaves the column:
/// the tab and the flag never fall back to the session id.
#[test]
fn a_dm_view_keeps_its_agents_name_when_the_agent_leaves_the_column() {
    fixture();
    let (tx, rx) = std::sync::mpsc::channel();
    let mut c = Chat::new(Some(rx), Palette::default(), Shape::PLAIN).sidebar(true).heartbeat(false).enhanced(true).dry_run(true).hidden_file(own_hidden());
    let dm = Signal { tray: "aaron".into(), ..sig("s-3", &format!("claude:{ALPHA_SID}"), ALPHA_CWD, "alpha to you", Channel::Person) };
    tx.send(dm.into()).unwrap();
    c.drain();
    side_click(&mut c, "alpha");
    let name = format!("@{}", nick(ALPHA_CWD));
    // A mark hides alpha's only message: alpha leaves the column.
    tx.send(attend_chat::watcher::Delivery::Withdrawn(vec![("aaron".into(), "s-3".into())])).unwrap();
    c.drain();
    assert!(c.signals().is_empty());
    assert!(!column(&mut c).iter().any(|r| r.contains("alpha")), "alpha has left the column: {}", screen(&mut c));
    let rows = rows(&mut c);
    assert!(rows[0].contains(&format!("{name} DMs")), "the tab keeps the name: {}", rows[0]);
    assert!(!rows[0].contains(ALPHA_SID), "{}", rows[0]);
    assert!(rows.iter().any(|r| r.contains(&format!(" {name} "))), "so does the flag: {rows:#?}");
}

/// The row of alpha's own DM view: its name and `DMs`, under alpha's row.
fn review_row(c: &mut Chat) -> Option<usize> {
    let name = nick(ALPHA_CWD);
    column(c).iter().position(|r| r.contains(&format!("{name} DMs")))
}

/// Pick Review DMs on alpha's menu, opened by a right click on its row.
fn review(c: &mut Chat) {
    let rows = rows(c);
    let (x, y) = rows.iter().enumerate().skip(1).find_map(|(y, r)| r.split('│').next()?.find("alpha").map(|b| (b as u16, y as u16))).unwrap();
    c.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Right), column: x, row: y, modifiers: KeyModifiers::NONE });
    assert!(items(c).iter().any(|i| i == "Review DMs"), "{}", screen(c));
    pick(c, "Review DMs");
}

/// Review DMs, on an agent's menu, lists the agent's own DM view under its
/// row and shows it: every DM the agent sent or received, with anyone, its
/// DMs with the operator among them, and nothing else.
#[test]
fn review_dms_lists_every_dm_the_agent_sent_or_received() {
    let mut c = chat();
    review(&mut c);
    let s = screen(&mut c);
    assert_eq!(c.foreground(), &Tab::Review(ALPHA_SID.into()), "{s}");
    let agent = row_index(&mut c, "alpha");
    assert_eq!(review_row(&mut c), Some(agent + 1), "listed under alpha's row: {:#?}", column(&mut c));
    let tabs = &rows(&mut c)[0];
    assert!(tabs.contains(&format!("{} DMs", nick(ALPHA_CWD))), "the tab bar shows the view: {tabs}");
    let f = feed(&mut c);
    for want in ["alpha to you", "you to alpha", "alpha to beta", "beta to alpha"] {
        assert!(f.contains(want), "{want}: {f}");
    }
    for to in [nick(ALPHA_CWD), nick(BETA_CWD)] {
        assert!(f.contains(&format!("→ @{to}")), "each DM between the agents names whom it went to: {f}");
    }
    for not in ["beta to you", "you to beta", "alpha on open", "beta on open", "alpha in deploy"] {
        assert!(!f.contains(not), "{not} is not alpha's DM: {f}");
    }
}

/// Both views listed: the operator's DMs with alpha, then alpha's own.
#[test]
fn both_views_are_listed_under_the_agent_in_order() {
    let mut c = chat();
    side_click(&mut c, "alpha");
    review(&mut c);
    let agent = row_index(&mut c, "alpha");
    let col = column(&mut c);
    assert!(col[agent + 1].contains("DMs") && !col[agent + 1].contains(&nick(ALPHA_CWD)), "the operator's view first: {col:#?}");
    assert_eq!(review_row(&mut c), Some(agent + 2), "{col:#?}");
}

/// The agent's own view offers nothing to send: no compose box, a typed
/// key writes nothing, and the focus ring has no stop for a box.
#[test]
fn the_agents_own_view_has_no_compose_box() {
    let mut c = chat_with(true);
    review(&mut c);
    let s = screen(&mut c);
    assert!(!rows(&mut c).iter().any(|r| r.contains("│> ") || r.contains("│ > ")), "no compose prompt: {s}");
    typed(&mut c, "hello");
    assert!(c.input().is_empty(), "nothing typed: {}", screen(&mut c));
    assert!(!c.signals().iter().any(|s| s.message == "hello"), "nothing sent");
    let mut seen = Vec::new();
    for _ in 0..40 {
        press(&mut c, KeyCode::Tab);
        seen.push(focused(&mut c));
    }
    assert!(!seen.iter().any(|f| f == "mode chat"), "no compose stop in the ring: {seen:?}");
    assert!(seen.iter().any(|f| f == "mode feed"), "the feed is: {seen:?}");
}

/// The agent's own view's row and tab carry a mark of their own, apart
/// from the operator's DM view's row, on the same info ground.
#[test]
fn the_agents_own_view_is_marked_apart() {
    let mut c = chat();
    side_click(&mut c, "alpha");
    review(&mut c);
    let rows = rows(&mut c);
    let name = nick(ALPHA_CWD);
    assert!(rows[0].contains('◈') && rows[0].contains(&format!("{name} DMs")), "the tab: {}", rows[0]);
    let col = column(&mut c);
    let review = col.iter().find(|r| r.contains(&format!("{name} DMs"))).unwrap();
    let dms = col.iter().find(|r| r.contains("DMs") && !r.contains(&name)).unwrap();
    assert!(review.contains('◈') && !dms.contains('◈'), "{col:#?}");
}

/// Hiding the agent's own view takes its row off the column and deletes
/// nothing; the agent's menu offers it again.
#[test]
fn hiding_the_agents_own_view_keeps_its_messages() {
    let mut c = chat();
    let before = c.signals().len();
    review(&mut c);
    let name = nick(ALPHA_CWD);
    side_click(&mut c, &format!("{name} DMs"));
    assert!(items(&mut c).iter().any(|i| i == "Hide review"), "{}", screen(&mut c));
    pick(&mut c, "Hide review");
    assert_eq!(review_row(&mut c), None, "the row is gone: {}", screen(&mut c));
    assert_eq!(c.foreground(), &Tab::Merged);
    assert_eq!(c.signals().len(), before, "nothing deleted");
    review(&mut c);
    assert!(feed(&mut c).contains("alpha to beta"));
}

/// A paste in the agent's own view has no box to land in: nothing is
/// pasted, and the status says so.
#[test]
fn a_paste_in_the_agents_own_view_pastes_nothing() {
    let mut c = chat();
    review(&mut c);
    agent_tui::screen::Screen::paste(&mut c, "a pasted line");
    assert!(c.input().is_empty(), "{}", screen(&mut c));
    assert_eq!(c.status(), "read only: nothing pasted");
}

/// Address on the agent's menu, from its own view, shows the operator's
/// DMs with it first, so the address lands in a box that is shown.
#[test]
fn address_from_the_agents_own_view_goes_to_its_dms() {
    let mut c = chat();
    review(&mut c);
    let rows = rows(&mut c);
    let (x, y) = rows.iter().enumerate().skip(1).find_map(|(y, r)| r.split('│').next()?.find("alpha").map(|b| (b as u16, y as u16))).unwrap();
    c.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Right), column: x, row: y, modifiers: KeyModifiers::NONE });
    pick(&mut c, "Address");
    assert_eq!(c.foreground(), &Tab::Dm(ALPHA_SID.into()));
    assert_eq!(c.input().text(), format!("@{} ", nick(ALPHA_CWD)));
}

/// The message menu in the agent's own view: Quote is set back, and Reply
/// and Address on a DM go to the operator's DMs with its sender first.
#[test]
fn the_message_menu_in_the_agents_own_view_writes_only_where_a_box_is() {
    for (item, want) in [("Reply", true), ("Address", true)] {
        let mut c = chat();
        review(&mut c);
        // The keyboard is on the feed: select the newest message, beta's DM
        // to alpha, and open its menu.
        press(&mut c, KeyCode::Up);
        press(&mut c, KeyCode::Enter);
        let menu_items = items(&mut c);
        assert!(menu_items.iter().any(|i| i.starts_with("Quote") && i.contains("read only")), "Quote set back: {menu_items:?}");
        pick(&mut c, item);
        let s = screen(&mut c);
        assert_eq!(c.foreground(), &Tab::Dm(BETA_SID.into()), "{item}: {s}");
        assert_eq!(c.input().text().starts_with(&format!("@{}", nick(BETA_CWD))), want, "{item}: {:?}", c.input().text());
    }
}
