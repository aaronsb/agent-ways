//! Messages held after Enter (ADR-409, note of 2026-11-01), through the
//! shell's real key and mouse handlers and its exit paths: the route a
//! message joins on, a channel dissolved during the countdown, a send
//! refused at its end, quitting and the unasked end. In its own file, so
//! its own fixture, on a bus the sends really write: alpha and beta live,
//! gamma quiet; #deploy with alpha in it; #gone to #gone4, which one test
//! each dissolves.

mod support;

use std::time::{Duration, UNIX_EPOCH};

use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use agent_tui::screen::Screen;
use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;
use attend_chat::signal::{self, Channel, Signal};
use attend_chat::tabs::Tab;

const ALPHA_SID: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const BETA_SID: &str = "bbbbbbbb-0000-4000-8000-000000000002";
/// Its heartbeat an hour old: quiet, so a send to it is refused.
const GAMMA_SID: &str = "cccccccc-0000-4000-8000-000000000003";
const W: u16 = 120;
const H: u16 = 35;
/// 2026-10-02 14:00 UTC.
const NOW: u64 = 1_790_949_600;

fn cwd(name: &str) -> String {
    format!("/work/{name}")
}

fn nick(name: &str) -> String {
    agent_identity::Identity::for_cwd(&cwd(name), agent_theme::ColorDepth::NoColor).nickname.to_string()
}

fn fixture() {
    support::fixture("pending", |base| {
        let home = std::path::PathBuf::from(std::env::var("HOME").unwrap());
        std::fs::create_dir_all(home.join(".claude/sessions")).unwrap();
        for (i, (sid, name)) in [(ALPHA_SID, "alpha"), (BETA_SID, "beta"), (GAMMA_SID, "gamma")].into_iter().enumerate() {
            let record = format!("{{\"pid\":1,\"sessionId\":\"{sid}\",\"cwd\":\"{}\"}}\n", cwd(name));
            std::fs::write(home.join(format!(".claude/sessions/{i}.json")), record).unwrap();
        }
        support::pin_alive(ALPHA_SID);
        support::pin_alive(BETA_SID);
        attend_presence::heartbeat::touch(GAMMA_SID).unwrap();
        let beat = std::fs::File::options().write(true).open(attend_presence::heartbeat::heartbeat_path(GAMMA_SID)).unwrap();
        beat.set_modified(std::time::SystemTime::now() - Duration::from_secs(3600)).unwrap();
        for c in ["deploy", "gone", "gone2", "gone3", "gone4", "gone5"] {
            attend_groups::Groups::new(base, ALPHA_SID).join(c, true).unwrap();
        }
    });
}

/// A chat on the fixture's bus whose Enter holds a message five seconds,
/// on a pinned clock that [`wait`] moves.
fn chat() -> Chat {
    fixture();
    let mut c = Chat::new(None, Palette::default(), Shape::PLAIN)
        .sidebar(true)
        .heartbeat(false)
        .enhanced(true)
        .question_arm(Duration::ZERO)
        .pinned(UNIX_EPOCH + Duration::from_secs(NOW), 0)
        .send_delay(Duration::from_secs(5));
    c.push(sig("s-1", "a message so the feed has one"));
    c
}

fn sig(id: &str, message: &str) -> Signal {
    Signal {
        id: id.into(),
        from: format!("claude:{ALPHA_SID}"),
        project: "alpha".into(),
        cwd: cwd("alpha"),
        reply_to: None,
        message: message.into(),
        ts: NOW - 600,
        channel: Channel::Open,
        tray: String::new(),
        beside_post: false,
    }
}

fn rows(c: &mut Chat) -> Vec<String> {
    testkit::rows(&testkit::render_screen(c, W, H))
}

fn screen(c: &mut Chat) -> String {
    rows(c).join("\n")
}

fn press(c: &mut Chat, code: KeyCode, m: KeyModifiers) -> bool {
    testkit::render_screen(c, W, H);
    testkit::drive(c, &[KeyEvent::new(code, m)])
}

fn key(c: &mut Chat, code: KeyCode) -> bool {
    press(c, code, KeyModifiers::NONE)
}

fn typed(c: &mut Chat, s: &str) {
    for ch in s.chars() {
        key(c, KeyCode::Char(ch));
    }
}

fn send(c: &mut Chat, s: &str) {
    typed(c, s);
    key(c, KeyCode::Enter);
}

/// Time passing on the pinned clock, and the tick that sends what is due.
fn wait(c: &mut Chat, secs: u64) {
    c.advance(Duration::from_secs(secs));
    Screen::tick(c);
}

/// Show `tab` by the column's Ctrl+digit, whichever number it has; 0 is
/// the operator view. A digit is pressed only while another tab is shown:
/// on the tab shown it opens its menu.
fn show(c: &mut Chat, tab: Tab) {
    for d in ['1', '2', '3', '4', '5', '6', '7', '8', '9', '0'] {
        if *c.foreground() == tab {
            return;
        }
        press(c, KeyCode::Char(d), KeyModifiers::CONTROL);
        if *c.foreground() == tab {
            return;
        }
    }
    panic!("no Ctrl+digit shows {tab:?}: {}", screen(c));
}

/// A left click on the first `text` in the left column.
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

/// A left click on `text` right of the column: a waiting message's body.
fn feed_click(c: &mut Chat, text: &str) {
    let buf = testkit::render_screen(c, W, H);
    let rows = testkit::rows(&buf);
    let (x, y) = rows
        .iter()
        .enumerate()
        .rev()
        .find_map(|(y, r)| {
            let (col, rest) = r.split_once('│')?;
            rest.find(text).map(|b| ((col.chars().count() + 1 + rest[..b].chars().count()) as u16, y as u16))
        })
        .unwrap_or_else(|| panic!("no {text} in the feed: {rows:#?}"));
    c.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE });
}

/// Whether any message on the bus under `dir` reads `body`.
fn written(dir: &std::path::Path, body: &str) -> bool {
    walk(dir).iter().any(|p| std::fs::read_to_string(p).is_ok_and(|t| t.contains(body)))
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

fn on_open(body: &str) -> bool {
    written(&signal::broadcast_dir(), body)
}

fn anywhere(body: &str) -> bool {
    written(&signal::signals_base(), body)
}

fn dissolve(channel: &str) {
    attend_groups::Groups::new(&signal::signals_base(), ALPHA_SID).dissolve(channel).unwrap();
}

// ── the route a message joins on ────────────────────────────────

/// The operator view and `#open` send to one place, so a message from
/// each joins one waiting.
#[test]
fn the_operator_view_and_open_share_one_message() {
    let mut c = chat().dry_run(true);
    send(&mut c, "from the operator view");
    show(&mut c, Tab::Channel("open".into()));
    send(&mut c, "from #open");
    assert_eq!(c.pending(), ["from the operator view\nfrom #open"]);
}

/// An unaddressed line in an agent's DM view goes to that agent; it
/// joins another from the same view and waits apart from `@Name` typed
/// elsewhere, which is sent by name.
#[test]
fn a_dm_views_lines_join_and_wait_apart_from_an_address() {
    let mut c = chat().dry_run(true);
    side_click(&mut c, &nick("alpha"));
    assert_eq!(c.foreground(), &Tab::Dm(ALPHA_SID.into()));
    send(&mut c, "in alpha's view");
    show(&mut c, Tab::Merged);
    send(&mut c, &format!("@{} by name", nick("alpha")));
    side_click(&mut c, &nick("alpha"));
    send(&mut c, "again in the view");
    assert_eq!(c.pending(), ["in alpha's view\nagain in the view".to_string(), format!("@{} by name", nick("alpha"))]);
}

/// `@Name` in a channel is a post there addressed to them: apart from the
/// channel's own posts, and apart from `#deploy @Name` typed anywhere,
/// which is a post and a direct message. In either order.
#[test]
fn at_name_in_a_channel_waits_apart_from_the_channel_and_from_an_explicit_run() {
    let alpha = nick("alpha");
    let mut c = chat().dry_run(true);
    show(&mut c, Tab::Channel("deploy".into()));
    send(&mut c, "to the channel");
    send(&mut c, &format!("@{alpha} in the channel"));
    assert_eq!(c.pending().len(), 2, "{:?}", c.pending());
    show(&mut c, Tab::Merged);
    send(&mut c, &format!("#deploy @{alpha} post and DM"));
    assert_eq!(c.pending().len(), 3, "the explicit run waits on its own: {:?}", c.pending());
    // The other order.
    let mut c = chat().dry_run(true);
    send(&mut c, &format!("#deploy @{alpha} post and DM"));
    show(&mut c, Tab::Channel("deploy".into()));
    send(&mut c, &format!("@{alpha} in the channel"));
    assert_eq!(c.pending(), [format!("#deploy @{alpha} post and DM"), format!("@{alpha} in the channel")]);
}

/// A bare `/` has no route: it is a command being typed and runs at once.
#[test]
fn a_bare_slash_is_not_held() {
    let mut c = chat().dry_run(true);
    send(&mut c, "/");
    assert!(c.pending().is_empty(), "{:?}", c.pending());
}

// ── at the end of a countdown ───────────────────────────────────

/// A channel dissolved during the countdown: the message is not sent to
/// `#open` in its place. It stays in the feed, not sent, with the reason
/// on the status line, and Esc recalls it.
#[test]
fn a_message_held_for_a_channel_dissolved_meanwhile_is_not_sent_elsewhere() {
    let mut c = chat();
    show(&mut c, Tab::Channel("gone".into()));
    send(&mut c, "rollout plan is X");
    dissolve("gone");
    c.refresh();
    wait(&mut c, 5);
    assert!(!on_open("rollout plan is X"), "not broadcast in its place");
    assert!(!anywhere("rollout plan is X"), "nor written anywhere");
    assert_eq!(c.pending(), ["rollout plan is X"], "kept, not lost");
    assert_eq!(c.status(), "not sent: #gone is gone");
    assert!(screen(&mut c).contains("not sent"), "{}", screen(&mut c));
    wait(&mut c, 30);
    assert!(!anywhere("rollout plan is X"), "nothing sends it by itself");
    key(&mut c, KeyCode::Esc);
    assert_eq!((c.input().text(), c.pending().len()), ("rollout plan is X", 0), "Esc recalls it");
}

/// A send refused at the end of its countdown leaves the draft typed
/// since as it is: the caret where it was, and the draft's own first line
/// still routing it (the review's scenario A). The refused message waits
/// apart; recalled unchanged, Enter sends it anyway.
#[test]
fn a_refusal_at_the_end_of_a_countdown_leaves_the_draft_and_its_route() {
    let gamma = nick("gamma");
    let mut c = chat();
    send(&mut c, &format!("@{gamma} ping"));
    typed(&mut c, "#deploy unrelated");
    let caret = c.input().cursor();
    wait(&mut c, 5);
    assert!(!anywhere(&format!("@{gamma} ping")), "refused: gamma is quiet");
    assert_eq!((c.input().text(), c.input().cursor()), ("#deploy unrelated", caret), "the draft and its caret as they were");
    assert!(c.status().starts_with("not sent:") && c.status().contains("quiet"), "{}", c.status());
    assert_eq!(c.pending(), [format!("@{gamma} ping")]);
    // The draft goes where its own first line says, and waits apart.
    key(&mut c, KeyCode::Enter);
    assert_eq!(c.pending(), [format!("@{gamma} ping"), "#deploy unrelated".to_string()]);
    wait(&mut c, 5);
    assert!(written(&signal::signals_base().join("@deploy"), "#deploy unrelated"), "the draft went to #deploy");
    // The refused one, recalled into the empty box, is sent anyway.
    feed_click(&mut c, "ping");
    assert_eq!(c.input().text(), format!("@{gamma} ping"));
    assert!(c.pending().is_empty());
    key(&mut c, KeyCode::Enter);
    assert!(c.status().starts_with("sent"), "Enter on the recalled refusal sends anyway: {}", c.status());
    assert!(anywhere(&format!("@{gamma} ping")));
}

/// A message refused at the end of its countdown is recalled into the view
/// it was written in, so Enter sends it where it was held for, not to
/// `#open` from the operator view (the review's scenario B).
#[test]
fn a_refused_message_is_recalled_into_its_own_view() {
    let mut c = chat();
    side_click(&mut c, &nick("gamma"));
    assert_eq!(c.foreground(), &Tab::Dm(GAMMA_SID.into()));
    send(&mut c, "for gamma's eyes");
    show(&mut c, Tab::Merged);
    wait(&mut c, 5);
    assert_eq!(c.pending(), ["for gamma's eyes"], "refused: gamma is quiet");
    key(&mut c, KeyCode::Esc);
    assert_eq!(c.foreground(), &Tab::Dm(GAMMA_SID.into()), "back in gamma's DM view");
    assert_eq!(c.input().text(), "for gamma's eyes");
    key(&mut c, KeyCode::Enter);
    assert!(!on_open("for gamma's eyes"), "never to #open");
    assert!(anywhere("for gamma's eyes"), "sent to gamma anyway: {}", c.status());
}

/// A send at the end of a countdown leaves the feed where the operator
/// scrolled it.
#[test]
fn a_send_at_the_end_of_a_countdown_keeps_the_scroll() {
    let mut c = chat();
    for i in 0..40 {
        c.push(sig(&format!("s-scroll-{i}"), &format!("filler {i}")));
    }
    send(&mut c, "sent while reading back");
    key(&mut c, KeyCode::PageUp);
    let back = c.scroll();
    assert!(back > 0, "scrolled back");
    wait(&mut c, 5);
    assert!(on_open("sent while reading back"));
    assert_eq!(c.scroll(), back, "the feed stays where it was read");
}

/// How many times `text` is on the screen at `w`x`h`, and the screen.
fn shown(c: &mut Chat, w: u16, h: u16, text: &str) -> (usize, String) {
    let s = testkit::rows(&testkit::render_screen(c, w, h)).join("\n");
    (s.matches(text).count(), s)
}

/// At 80x11 the feed has three rows and a held message's header is out of
/// them: the status line counts down instead, with where it goes and the
/// key that recalls it, until it is sent.
#[test]
fn a_countdown_the_feed_cannot_show_is_on_the_status_line() {
    let mut c = chat();
    send(&mut c, "held in a short pane");
    let (n, s) = shown(&mut c, 80, 11, "sending in 5s → #open · Esc recall");
    assert_eq!(n, 1, "the countdown is on screen once:\n{s}");
    assert_eq!(s.matches("Esc recall").count(), 1, "the footer does not repeat the key:\n{s}");
    wait(&mut c, 2);
    let (n, s) = shown(&mut c, 80, 11, "sending in 3s → #open");
    assert_eq!(n, 1, "it counts down:\n{s}");
    wait(&mut c, 3);
    assert!(on_open("held in a short pane"));
    let (n, s) = shown(&mut c, 80, 11, "sending in");
    assert_eq!(n, 0, "sent, no countdown is left:\n{s}");
}

/// With several waiting and none on screen, the status line names the
/// soonest and how many wait.
#[test]
fn several_countdowns_off_screen_name_the_soonest() {
    let mut c = chat();
    send(&mut c, "first, to open");
    wait(&mut c, 2);
    show(&mut c, Tab::Channel("deploy".into()));
    send(&mut c, "second, to deploy");
    let (n, s) = shown(&mut c, 80, 11, "2 waiting, next in 3s → #open");
    assert_eq!(n, 1, "the soonest and the count:\n{s}");
}

/// At 80x25 the header is in the feed: nothing more is drawn, so the
/// countdown is not shown twice.
#[test]
fn a_countdown_the_feed_shows_is_not_repeated() {
    let mut c = chat();
    send(&mut c, "held in a tall pane");
    let (n, s) = shown(&mut c, 80, 25, "sending in 5s");
    assert_eq!(n, 1, "the feed's header alone:\n{s}");
    assert!(s.contains("Esc or click recalls"), "the one shown is the header:\n{s}");
    assert_eq!(s.matches("Esc recall").count(), 1, "the footer names the key:\n{s}");
}

/// Paged back past it at 80x25, the header is out of view and the status
/// line counts down.
#[test]
fn a_countdown_scrolled_out_of_view_is_on_the_status_line() {
    let mut c = chat();
    for i in 0..40 {
        c.push(sig(&format!("s-back-{i}"), &format!("filler {i}")));
    }
    send(&mut c, "held while reading back");
    testkit::render_screen(&mut c, 80, 25);
    testkit::drive(&mut c, &[KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE)]);
    let (n, s) = shown(&mut c, 80, 25, "sending in 5s → #open");
    assert_eq!(n, 1, "on the status line:\n{s}");
}

// ── quitting ─────────────────────────────────────────────────────

/// Send and quit with a message that cannot go: the chat stays open, the
/// message stays, and the status line says why.
#[test]
fn send_and_quit_keeps_the_chat_open_when_a_message_cannot_go() {
    let mut c = chat();
    show(&mut c, Tab::Channel("gone2".into()));
    send(&mut c, "too late for this one");
    dissolve("gone2");
    c.refresh();
    assert!(press(&mut c, KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(c.app().guarding());
    assert!(key(&mut c, KeyCode::Char('s')), "s did not quit: the message could not go");
    assert!(!c.app().guarding());
    assert_eq!(c.pending(), ["too late for this one"]);
    assert_eq!(c.status(), "not sent: #gone2 is gone");
    assert!(!anywhere("too late for this one"));
}

/// The guard's Send and quit names the draft in the compose box and keeps
/// it: the next chat opens with it in the box of the view it was written
/// in, and its file stays until the draft is held (N4).
#[test]
fn send_and_quit_keeps_the_draft_for_the_next_chat() {
    let kept = unsent("send-and-quit");
    let mut c = chat().unsent_dir(&kept);
    send(&mut c, "waiting at the quit");
    typed(&mut c, "a draft not yet sent");
    press(&mut c, KeyCode::Char('c'), KeyModifiers::CONTROL);
    let s = screen(&mut c);
    assert!(s.contains("Send and quit, keep the draft (s)") && s.contains("a draft in the compose box"), "{s}");
    assert!(!key(&mut c, KeyCode::Char('s')), "s sends and quits");
    assert!(on_open("waiting at the quit"));
    let mut c = chat().unsent_dir(&kept).restore_unsent();
    assert_eq!(c.input().text(), "a draft not yet sent", "the draft opens the next chat");
    assert_eq!(files(&kept).len(), 1, "its file stays, claimed, while the draft is in the box");
    press(&mut c, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(c.pending(), ["a draft not yet sent"]);
    assert!(files(&kept).is_empty(), "held again, its file goes");
}

/// A directory of the fixture's for text kept unsent, one per test. The
/// fixture is set up first: called before any [`chat`], the root would
/// otherwise be made here, with no setup.
fn unsent(tag: &str) -> std::path::PathBuf {
    fixture();
    let root = support::fixture("pending", |_| unreachable!("set up by fixture()"));
    root.join(format!("unsent-{tag}"))
}

/// The names of the files in `dir`.
fn files(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    v.sort();
    v
}

/// A signal or the terminal hanging up ends the chat with no one asked:
/// what waits is sent, once, through the shell's own exit path, and what
/// cannot go, with the draft, is kept for the next chat. The operator said:
/// "I don't want to drop messages". The next chat shows each as an entry of
/// its own: the draft's view, #gone3, is gone, so it is not in the box.
#[test]
fn the_unasked_end_sends_what_waits_and_keeps_what_cannot_go() {
    let kept = unsent("unasked");
    let mut c = chat().unsent_dir(&kept);
    send(&mut c, "said as the terminal closed");
    show(&mut c, Tab::Channel("gone3".into()));
    send(&mut c, "held for a channel about to go");
    typed(&mut c, "typed, not sent");
    dissolve("gone3");
    c.refresh();
    let session = c.into_app().end_unasked(libc::SIGHUP, "stopped: the terminal hung up");
    assert_eq!(session.signal, Some(libc::SIGHUP));
    assert!(on_open("said as the terminal closed"), "sent as the chat ended");
    assert!(!anywhere("held for a channel about to go"), "nothing sent elsewhere in a vanished channel's place");
    let c = chat().unsent_dir(&kept).restore_unsent();
    assert_eq!(c.input().text(), "", "nothing merged into the box");
    assert_eq!(c.pending(), ["held for a channel about to go", "typed, not sent"], "each an entry of its own");
    assert!(c.status().contains("restored: 2 kept texts") && c.status().contains("the view of the draft for #gone3 is gone"), "{}", c.status());
}

/// Two chats ending unasked at once each keep their own text: the next
/// chat opens with the second's draft in its box and the first's message
/// in the feed.
#[test]
fn two_chats_ending_at_once_keep_both_texts() {
    let kept = unsent("two");
    let mut first = chat().unsent_dir(&kept);
    show(&mut first, Tab::Channel("gone4".into()));
    send(&mut first, "the first chat's message");
    let mut second = chat().unsent_dir(&kept);
    typed(&mut second, "the second chat's draft");
    dissolve("gone4");
    first.refresh();
    first.into_app().end_unasked(libc::SIGHUP, "stopped: the terminal hung up");
    second.into_app().end_unasked(libc::SIGTERM, "stopped by a signal");
    assert_eq!(files(&kept).len(), 2, "a file each");
    let c = chat().unsent_dir(&kept).restore_unsent();
    assert_eq!(c.input().text(), "the second chat's draft");
    assert_eq!(c.pending(), ["the first chat's message"]);
    assert!(files(&kept).iter().all(|f| f.starts_with(".claim.")), "both claimed, neither removed: {:?}", files(&kept));
}

/// Two chats that each keep the same words keep a text each, and both
/// come back: the operator said "I don't want to drop messages" (the
/// review's N2).
#[test]
fn two_chats_that_keep_the_same_reply_get_both_back() {
    let kept = unsent("same-reply");
    let mut first = chat().unsent_dir(&kept);
    typed(&mut first, "yes");
    let mut second = chat().unsent_dir(&kept);
    typed(&mut second, "yes");
    first.into_app().end_unasked(libc::SIGHUP, "stopped: the terminal hung up");
    second.into_app().end_unasked(libc::SIGHUP, "stopped: the terminal hung up");
    assert_eq!(files(&kept).len(), 2, "a file each");
    let c = chat().unsent_dir(&kept).restore_unsent();
    assert_eq!((c.input().text(), c.pending()), ("yes", vec!["yes".to_string()]), "both back: {}", c.status());
    assert_eq!(files(&kept).len(), 2, "neither file removed: {:?}", files(&kept));
}

/// One chat's text in two files is one text: the copy is removed and the
/// text comes back once.
#[test]
fn one_chats_text_kept_twice_comes_back_once() {
    let kept = unsent("one-writer");
    let mut c = chat().unsent_dir(&kept);
    typed(&mut c, "kept twice by one chat");
    c.into_app().end_unasked(libc::SIGHUP, "stopped: the terminal hung up");
    let name = files(&kept).remove(0);
    std::fs::copy(kept.join(&name), kept.join(format!("{name}9"))).unwrap();
    let c = chat().unsent_dir(&kept).restore_unsent();
    assert_eq!(c.input().text(), "kept twice by one chat");
    assert!(c.pending().is_empty(), "once: {:?}", c.pending());
    assert_eq!(files(&kept).len(), 1, "the copy removed: {:?}", files(&kept));
}

/// A third signal can cut short a forced exit's copy of a restored text,
/// while the file the text came from is still whole: the copy names that
/// file as the one it carries on, so the next chat drops the cut copy and
/// restores the text once, whole. A copy cut short while the text was
/// being sent leaves its mark: the text comes back marked "may have been
/// sent".
#[test]
fn a_cut_copy_of_a_kept_text_is_dropped_for_the_whole_one() {
    for sending in [false, true] {
        let kept = unsent(if sending { "cut-copy-sending" } else { "cut-copy" });
        let mut c = chat().unsent_dir(&kept);
        typed(&mut c, "the whole text, carried on");
        c.into_app().end_unasked(libc::SIGHUP, "stopped: the terminal hung up");
        let origin = files(&kept).remove(0);
        let body = std::fs::read_to_string(kept.join(&origin)).unwrap();
        let writer = body.lines().find_map(|l| l.strip_prefix("writer: ")).unwrap().to_string();
        let mut copy = body.replace(&format!("writer: {writer}"), "writer: 999999999.1.0").replace("from: \n", &format!("from: {origin}\n"));
        if sending {
            copy = copy.replace("kind: draft", "kind: sending");
        }
        std::fs::write(kept.join(".1-00000000000000000009-0.999999999.0.tmp"), &copy[..copy.len() - 8]).unwrap();
        let mut c = chat().unsent_dir(&kept).restore_unsent();
        assert!(!c.status().contains("cut short"), "no cut copy restored: {}", c.status());
        if sending {
            assert_eq!(c.input().text(), "", "never in the box: Enter there would send it");
            assert_eq!(c.pending(), ["the whole text, carried on"], "{}", c.status());
            assert!(screen(&mut c).contains("may have been sent"), "the mark carried over");
        } else {
            assert_eq!(c.input().text(), "the whole text, carried on", "{}", c.status());
            assert!(c.pending().is_empty(), "once: {:?}", c.pending());
        }
        let left = files(&kept);
        assert_eq!(left.len(), 1, "one file, holding the whole text: {left:?}");
        let held = std::fs::read_to_string(kept.join(&left[0])).unwrap();
        assert!(held.ends_with("\n\nthe whole text, carried on"), "{held}");
        assert_eq!(held.contains("kind: sending"), sending, "{held}");
    }
}

// ── restoring into the view written in ───────────────────────────

/// A draft written in a DM view opens the next chat in that view, its
/// flag the agent's: never in the default view, bound for #open (N2).
#[test]
fn a_dm_draft_comes_back_in_its_own_dm_view() {
    let kept = unsent("dm-draft");
    let mut c = chat().unsent_dir(&kept);
    side_click(&mut c, &nick("alpha"));
    typed(&mut c, "for alpha only: staging creds rotate tonight");
    c.into_app().end_unasked(libc::SIGHUP, "stopped: the terminal hung up");
    let mut c = chat().unsent_dir(&kept).restore_unsent();
    assert_eq!(c.foreground(), &Tab::Dm(ALPHA_SID.into()), "shown in the DM view it was written in");
    assert_eq!(c.input().text(), "for alpha only: staging creds rotate tonight");
    press(&mut c, KeyCode::Enter, KeyModifiers::NONE);
    wait(&mut c, 5);
    assert!(!on_open("staging creds"), "never posted to #open");
}

/// A message held for a channel dissolved before the end is an entry;
/// recalled, it comes back alone, and the box keeps the operator's own
/// draft apart: a recall into the shown view puts it before the draft.
#[test]
fn a_kept_message_is_an_entry_recalled_on_its_own() {
    let kept = unsent("kept-entry");
    let mut c = chat().unsent_dir(&kept);
    show(&mut c, Tab::Channel("gone5".into()));
    send(&mut c, "plan for gone");
    dissolve("gone5");
    c.refresh();
    c.into_app().end_unasked(libc::SIGHUP, "stopped: the terminal hung up");
    let mut c = chat().unsent_dir(&kept).restore_unsent();
    assert_eq!(c.input().text(), "");
    assert_eq!(c.pending(), ["plan for gone"]);
    assert!(screen(&mut c).contains("not sent"), "an entry marked not sent");
    key(&mut c, KeyCode::Esc);
    assert_eq!(c.input().text(), "plan for gone");
    assert!(c.pending().is_empty());
    assert_eq!(files(&kept).len(), 1, "claimed until the text goes");
}

/// A restored text is not sent by the unasked end, and its file survives
/// it (N4): the chat after that restores it again.
#[test]
fn a_restored_text_survives_the_next_unasked_end() {
    let kept = unsent("survives");
    let mut c = chat().unsent_dir(&kept);
    typed(&mut c, "a draft that outlives two chats");
    c.into_app().end_unasked(libc::SIGHUP, "stopped: the terminal hung up");
    let c = chat().unsent_dir(&kept).restore_unsent();
    assert_eq!(c.input().text(), "a draft that outlives two chats");
    c.into_app().end_unasked(libc::SIGHUP, "stopped: the terminal hung up");
    let c = chat().unsent_dir(&kept).restore_unsent();
    assert_eq!(c.input().text(), "a draft that outlives two chats", "kept again by the second chat's end");
    assert_eq!(files(&kept).len(), 1, "one file: the old one removed once the new one was written");
}

/// A file that cannot be read stays where it is, named on the status line;
/// the good one beside it is restored (N5).
#[test]
fn an_unreadable_file_is_left_in_place_and_named() {
    let kept = unsent("unreadable");
    let mut c = chat().unsent_dir(&kept);
    typed(&mut c, "the readable one");
    c.into_app().end_unasked(libc::SIGHUP, "stopped: the terminal hung up");
    std::fs::write(kept.join("1-00000000000000000001-0"), [0xff, 0xfe, 0x00, 0x41]).unwrap();
    let c = chat().unsent_dir(&kept).restore_unsent();
    assert_eq!(c.input().text(), "the readable one");
    assert!(files(&kept).contains(&"1-00000000000000000001-0".to_string()), "left under its own name: {:?}", files(&kept));
    assert!(c.status().contains("not restored (unreadable, left in place): 1-00000000000000000001-0"), "{}", c.status());
}

/// A claim is live only while its lock is held (#1240 item 3): a claim
/// whose lock another open holds is left to it, and one with no lock is
/// claimed again, whatever its pid, a live pid that another process now
/// has included. A temporary file a crashed writer left is restored when
/// whole, and as a draft marked cut short when cut short.
#[test]
fn claims_and_crashed_writes_are_sorted_out_at_start() {
    let kept = unsent("claims");
    let mut c = chat().unsent_dir(&kept);
    typed(&mut c, "a draft to copy");
    c.into_app().end_unasked(libc::SIGHUP, "stopped: the terminal hung up");
    let name = files(&kept).remove(0);
    let body = std::fs::read_to_string(kept.join(&name)).unwrap();
    std::fs::remove_file(kept.join(&name)).unwrap();
    let body_of = |t: &str| body.replace("a draft to copy", t).replace("bytes: 15", &format!("bytes: {}", t.len()));
    // Held: another open of the file holds its lock, as a running chat does.
    let held = kept.join(".claim.999999998.1-00000000000000000000-0");
    std::fs::write(&held, body_of("held by a running chat")).unwrap();
    let lock = std::fs::File::open(&held).unwrap();
    // SAFETY: flock on a descriptor this File owns.
    assert_eq!(unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&lock), libc::LOCK_EX | libc::LOCK_NB) }, 0);
    // pid 1 runs, but holds no lock on this file: its pid was reused.
    std::fs::write(kept.join(".claim.1.1-00000000000000000001-0"), body_of("claimed under a reused pid")).unwrap();
    std::fs::write(kept.join(".claim.999999999.1-00000000000000000002-0"), body_of("left by a gone chat")).unwrap();
    std::fs::write(kept.join(".1-00000000000000000003-0.999999999.0.tmp"), body_of("written whole")).unwrap();
    let cut = body_of("cut short by a crash");
    std::fs::write(kept.join(".1-00000000000000000004-0.999999999.1.tmp"), &cut[..cut.len() - 5]).unwrap();
    let c = chat().unsent_dir(&kept).restore_unsent();
    assert_eq!(c.input().text(), "written whole", "the newest whole draft, in the box: {}", c.status());
    assert_eq!(c.pending(), ["claimed under a reused pid", "left by a gone chat", "cut short by a "]);
    assert!(c.status().contains("one was cut short by a crash"), "{}", c.status());
    let mut c = c;
    assert!(screen(&mut c).contains("draft, cut short"), "the cut-short entry is marked");
    let left = files(&kept);
    assert!(left.contains(&".claim.999999998.1-00000000000000000000-0".to_string()), "a claim whose lock is held is untouched: {left:?}");
    assert!(!left.iter().any(|f| f.ends_with(".tmp")), "no temporary file left: {left:?}");
    drop(lock);
}

/// A text that was being sent as a chat ended is restored marked so, and
/// never sent by itself (#1240 item 2): the forced exit or panic that kept
/// it could not tell whether the send had gone.
#[test]
fn a_text_kept_while_being_sent_is_marked_may_have_been_sent() {
    let kept = unsent("sending");
    let mut c = chat().unsent_dir(&kept);
    typed(&mut c, "the body");
    c.into_app().end_unasked(libc::SIGHUP, "stopped: the terminal hung up");
    let name = files(&kept).remove(0);
    let body = std::fs::read_to_string(kept.join(&name)).unwrap().replace("kind: draft", "kind: sending");
    std::fs::write(kept.join(&name), body).unwrap();
    let mut c = chat().unsent_dir(&kept).restore_unsent();
    assert_eq!(c.input().text(), "", "never in the box: Enter there would send it");
    assert_eq!(c.pending(), ["the body"]);
    assert!(c.status().contains("may have gone: check before Enter"), "{}", c.status());
    assert!(screen(&mut c).contains("may have been sent"), "the entry is marked");
    wait(&mut c, 60);
    assert!(!on_open("the body"), "nothing sends it by itself");
}

/// A claimed file whose lock cannot be tried stays where it is and is
/// named on the status line, never hidden (#1240 item 3).
#[test]
fn a_claim_that_cannot_be_checked_is_named() {
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: geteuid reads our own id.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipped: root opens a file of mode 000");
        return;
    }
    let kept = unsent("unjudged");
    std::fs::create_dir_all(&kept).unwrap();
    let claimed = kept.join(".claim.999999999.1-00000000000000000001-0");
    std::fs::write(&claimed, "text no one can open").unwrap();
    std::fs::set_permissions(&claimed, std::fs::Permissions::from_mode(0o000)).unwrap();
    let c = chat().unsent_dir(&kept).restore_unsent();
    std::fs::set_permissions(&claimed, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(c.status().starts_with("not restored (its claim could not be checked: Permission denied"), "{}", c.status());
    assert!(c.status().contains(".claim.999999999.1-00000000000000000001-0"), "{}", c.status());
    assert!(claimed.exists(), "left in place");
}

/// An earlier build's single file put aside as `<dir>.moving.<pid>` by a
/// chat that ended before moving it in is moved in by the next (#1240).
#[test]
fn a_file_left_aside_while_moving_in_is_restored() {
    let kept = unsent("moving");
    std::fs::create_dir_all(&kept).unwrap();
    let aside = kept.with_extension("moving.999999999");
    std::fs::write(&aside, "typed before views were noted\n").unwrap();
    let c = chat().unsent_dir(&kept).restore_unsent();
    assert!(!aside.exists(), "moved in");
    assert_eq!(c.pending(), ["typed before views were noted"], "{}", c.status());
}

/// The quit guard names everything unsaved at 80 columns: a long note
/// wraps under itself rather than being cut at the dialog's edge (#1240).
#[test]
fn the_quit_guard_names_everything_at_80_columns() {
    let mut c = chat();
    send(&mut c, "an open post");
    side_click(&mut c, &nick("beta"));
    send(&mut c, "for beta");
    typed(&mut c, "and a draft in the box");
    press(&mut c, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(c.app().guarding());
    let note = "a draft in the compose box and 2 messages waiting to send";
    let s = testkit::text(&testkit::render_screen(&mut c, 80, 25));
    let shown: Vec<&str> = s.split_whitespace().collect();
    for w in note.split_whitespace() {
        assert!(shown.contains(&w), "{w:?} of {note:?} is cut from the guard:\n{s}");
    }
}

/// With drafts and no message waiting, the guard offers Quit, keep the
/// drafts, so drafts are never left with only discard to quit by; that is
/// the case after a Send and quit whose message went but whose draft could
/// not be kept (#1235 review, L1).
#[test]
fn the_guard_keeps_drafts_when_no_message_waits() {
    let kept = unsent("keep-drafts");
    let mut c = chat().unsent_dir(&kept);
    typed(&mut c, "a draft alone");
    press(&mut c, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(c.app().guarding());
    let s = screen(&mut c);
    assert!(s.contains("Quit, keep the draft (k)"), "{s}");
    // At 80 columns the three buttons take two rows rather than one cut
    // at the guard's edge.
    let narrow = testkit::text(&testkit::render_screen(&mut c, 80, 25));
    assert!(narrow.contains("Quit, keep the draft (k)") && narrow.contains("Quit and discard all (D)"), "{narrow}");
    assert!(!key(&mut c, KeyCode::Char('k')), "k keeps the draft and quits");
    let c = chat().unsent_dir(&kept).restore_unsent();
    assert_eq!(c.input().text(), "a draft alone", "the next chat opens on it");
}

#[test]
fn after_a_send_and_quit_that_could_not_keep_the_draft_the_guard_still_keeps_it() {
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: geteuid reads our own id.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipped: root writes to a read-only directory");
        return;
    }
    let kept = unsent("keep-after-fail");
    let mut c = chat().unsent_dir(&kept);
    send(&mut c, "goes at the quit");
    typed(&mut c, "the draft that stays");
    std::fs::create_dir_all(&kept).unwrap();
    std::fs::set_permissions(&kept, std::fs::Permissions::from_mode(0o555)).unwrap();
    press(&mut c, KeyCode::Char('c'), KeyModifiers::CONTROL);
    let stayed = key(&mut c, KeyCode::Char('s'));
    std::fs::set_permissions(&kept, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(stayed, "the draft could not be kept: the chat stays");
    assert!(on_open("goes at the quit"), "the message went");
    press(&mut c, KeyCode::Char('c'), KeyModifiers::CONTROL);
    let s = screen(&mut c);
    assert!(s.contains("Quit, keep the draft (k)") && !s.contains("Send and quit"), "{s}");
    assert!(!key(&mut c, KeyCode::Char('k')), "k keeps the draft and quits");
    let c = chat().unsent_dir(&kept).restore_unsent();
    assert_eq!(c.input().text(), "the draft that stays");
    assert_eq!(files(&kept).len(), 1, "{:?}", files(&kept));
}

#[test]
#[ignore = "run by a_panic_after_a_partly_failed_keep_keeps_the_box_draft_once"]
fn child_panics_after_a_partly_failed_keep() {
    let Some(kept) = std::env::var_os("ATTEND_CHAT_PARTIAL_DIR").map(std::path::PathBuf::from) else { return };
    let mut c = chat().unsent_dir(&kept);
    attend_chat::app::keep_on_panic();
    send(&mut c, "held, sent at the quit");
    side_click(&mut c, &nick("beta"));
    typed(&mut c, &"a long parked draft ".repeat(200));
    // Esc recalls the held message into its view and parks the long draft.
    key(&mut c, KeyCode::Esc);
    press(&mut c, KeyCode::Enter, KeyModifiers::NONE);
    typed(&mut c, "the box draft");
    // A file size limit fails the long draft's write and lets the short
    // one through: a keep that partly fails.
    // SAFETY: setrlimit and signal on this process alone.
    unsafe {
        libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
        let lim = libc::rlimit { rlim_cur: 1024, rlim_max: libc::RLIM_INFINITY };
        assert_eq!(libc::setrlimit(libc::RLIMIT_FSIZE, &lim), 0);
    }
    press(&mut c, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(key(&mut c, KeyCode::Char('s')), "the long draft could not be kept: the chat stays");
    assert!(c.status().starts_with("not kept ("), "{}", c.status());
    panic!("a panic after the partly failed keep");
}

/// A panic right after a keep that partly failed writes the box draft no
/// second time (#1235 review, L2): what is noted follows the keep.
#[test]
fn a_panic_after_a_partly_failed_keep_keeps_the_box_draft_once() {
    let kept = unsent("partial-keep");
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["child_panics_after_a_partly_failed_keep", "--exact", "--ignored", "--test-threads=1"])
        .env("ATTEND_CHAT_PARTIAL_DIR", &kept)
        .output()
        .unwrap();
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(said.contains("a panic after the partly failed keep"), "the child reached its panic: {said}");
    let bodies: Vec<String> = files(&kept).iter().filter_map(|f| std::fs::read_to_string(kept.join(f)).ok()).collect();
    assert_eq!(bodies.iter().filter(|b| b.ends_with("the box draft")).count(), 1, "the box draft once: {bodies:?}");
}

// ── a recall that shows another view ─────────────────────────────

/// A recall that shows the message's view parks the draft in the box as a
/// draft of the view it was written in (N3): the recalled message alone in
/// the box, and the draft back when its view is shown with the box empty.
#[test]
fn a_recall_into_another_view_parks_the_draft_in_its_own() {
    let mut c = chat();
    send(&mut c, "an open post");
    side_click(&mut c, &nick("beta"));
    typed(&mut c, "for beta only");
    key(&mut c, KeyCode::Esc);
    assert_eq!(c.foreground(), &Tab::Merged, "shown where the message was written");
    assert_eq!(c.input().text(), "an open post", "the recalled message alone");
    assert_eq!(c.pending(), ["for beta only"], "the draft parked");
    assert!(screen(&mut c).contains(" draft "), "an entry marked draft");
    press(&mut c, KeyCode::Enter, KeyModifiers::NONE);
    side_click(&mut c, &nick("beta"));
    assert_eq!(c.foreground(), &Tab::Dm(BETA_SID.into()));
    assert_eq!(c.input().text(), "for beta only", "back in its view's box");
    assert_eq!(c.pending(), ["an open post"]);
}

/// A draft parked by a recall is named by the exit guard and kept by the
/// unasked end, in its own view.
#[test]
fn a_parked_draft_is_kept_at_the_end() {
    let kept = unsent("parked");
    let mut c = chat().unsent_dir(&kept);
    send(&mut c, "an open post to recall");
    side_click(&mut c, &nick("beta"));
    typed(&mut c, "parked for beta");
    key(&mut c, KeyCode::Esc);
    press(&mut c, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(screen(&mut c).contains("a draft written for"), "the guard names the parked draft");
    key(&mut c, KeyCode::Esc);
    c.into_app().end_unasked(libc::SIGHUP, "stopped: the terminal hung up");
    let c = chat().unsent_dir(&kept).restore_unsent();
    assert_eq!(c.foreground(), &Tab::Merged, "the box's draft, kept last, opens in its view");
    assert_eq!(c.input().text(), "an open post to recall");
    assert_eq!(c.pending(), ["parked for beta"], "the parked draft an entry of its own");
}

/// Send and quit sends what waits, and a parked draft whose file cannot be
/// written stays in the feed: the chat stays open, the status line says
/// why, and the guard still names the draft. Kept once the directory takes
/// it, it opens the next chat.
#[test]
fn a_parked_draft_that_cannot_be_kept_stays_in_the_feed() {
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: geteuid reads our own id.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipped: root writes to a read-only directory");
        return;
    }
    let kept = unsent("parked-unwritable");
    let mut c = chat().unsent_dir(&kept);
    send(&mut c, "held, then sent at the quit");
    side_click(&mut c, &nick("beta"));
    typed(&mut c, "parked and not lost");
    key(&mut c, KeyCode::Esc);
    press(&mut c, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(c.pending(), ["parked and not lost", "held, then sent at the quit"], "one draft parked and one message waiting");
    std::fs::create_dir_all(&kept).unwrap();
    std::fs::set_permissions(&kept, std::fs::Permissions::from_mode(0o555)).unwrap();
    press(&mut c, KeyCode::Char('c'), KeyModifiers::CONTROL);
    let stayed = key(&mut c, KeyCode::Char('s'));
    std::fs::set_permissions(&kept, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(stayed, "s did not quit: the draft could not be kept");
    assert!(on_open("held, then sent at the quit"), "the message went");
    assert!(c.status().starts_with("not kept (Permission denied"), "the reason first, so a narrow status line keeps it: {}", c.status());
    assert_eq!(c.pending(), ["parked and not lost"], "the draft stays in the feed");
    assert!(files(&kept).is_empty(), "nothing was written: {:?}", files(&kept));
    press(&mut c, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(c.app().guarding(), "the next Ctrl-C is guarded");
    assert!(screen(&mut c).contains("a draft written for"), "the guard names the draft");
    key(&mut c, KeyCode::Esc);
    c.into_app().end_unasked(libc::SIGHUP, "stopped: the terminal hung up");
    let c = chat().unsent_dir(&kept).restore_unsent();
    assert_eq!(c.input().text(), "parked and not lost", "kept at the end, it opens the next chat: {}", c.status());
}

// ── the delay set to 0 ───────────────────────────────────────────

/// With a message waiting, a delay set to 0 sends the next message to its
/// route with it at once, and the one after that at once on its own.
#[test]
fn a_delay_set_to_zero_sends_what_waits_with_the_next_message() {
    fixture();
    // No delay fixed for this chat: it reads the setting, which the
    // fixture home sets to 0 and this test to 5 first.
    let mut c = Chat::new(None, Palette::default(), Shape::PLAIN)
        .sidebar(true)
        .heartbeat(false)
        .enhanced(true)
        .pinned(UNIX_EPOCH + Duration::from_secs(NOW), 0);
    send(&mut c, "/config send_delay 5");
    send(&mut c, "zero first");
    assert_eq!(c.pending(), ["zero first"]);
    send(&mut c, "/config send_delay 0");
    assert_eq!(c.pending(), ["zero first"], "the command ran at once, and the message still waits");
    send(&mut c, "zero second");
    assert!(c.pending().is_empty(), "{:?}", c.pending());
    assert!(on_open("zero first\nzero second"), "joined and sent at once");
    send(&mut c, "zero third");
    assert!(c.pending().is_empty() && on_open("zero third"), "sent at once, as Enter did before the delay");
}
