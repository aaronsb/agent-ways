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

/// A directory of the fixture's for text kept unsent, one per test.
fn unsent(tag: &str) -> std::path::PathBuf {
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
    assert!(c.status().contains("not restored: 1-00000000000000000001-0"), "{}", c.status());
}

/// A file claimed by a chat still running is left to it; one claimed by a
/// chat no longer running is claimed again. A temporary file a crashed
/// writer left is restored when whole and removed when cut short.
#[test]
fn claims_and_crashed_writes_are_sorted_out_at_start() {
    let kept = unsent("claims");
    let mut c = chat().unsent_dir(&kept);
    typed(&mut c, "a draft to copy");
    c.into_app().end_unasked(libc::SIGHUP, "stopped: the terminal hung up");
    let name = files(&kept).remove(0);
    let body = std::fs::read_to_string(kept.join(&name)).unwrap();
    std::fs::remove_file(kept.join(&name)).unwrap();
    // pid 1 runs; 999999999 does not.
    let body_of = |t: &str| body.replace("a draft to copy", t).replace("bytes: 15", &format!("bytes: {}", t.len()));
    std::fs::write(kept.join(".claim.1.1-00000000000000000001-0"), body_of("held by a running chat")).unwrap();
    std::fs::write(kept.join(".claim.999999999.1-00000000000000000002-0"), body_of("left by a gone chat")).unwrap();
    std::fs::write(kept.join(".1-00000000000000000003-0.999999999.0.tmp"), body_of("written whole")).unwrap();
    let cut = body_of("cut short by a crash");
    std::fs::write(kept.join(".1-00000000000000000004-0.999999999.1.tmp"), &cut[..cut.len() - 5]).unwrap();
    let c = chat().unsent_dir(&kept).restore_unsent();
    assert_eq!(c.input().text(), "written whole", "the newest draft, in the box: {}", c.status());
    assert_eq!(c.pending(), ["left by a gone chat"]);
    let left = files(&kept);
    assert!(left.contains(&".claim.1.1-00000000000000000001-0".to_string()), "a running chat's claim untouched: {left:?}");
    assert!(!left.iter().any(|f| f.ends_with(".tmp")), "no temporary file left: {left:?}");
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
