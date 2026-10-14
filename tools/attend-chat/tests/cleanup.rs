//! The cleanup items (ADR-405, amendment of 2026-10-13): Hide messages
//! here, Delete read messages for everyone… and Tombstone…, through the
//! shell's real key and mouse handlers. The attend CLI is a fake `attend`
//! put first on `PATH`, which records each argv and answers as attend's
//! `--json` does. In its own file, so its own fixture and `PATH`: alpha
//! live, gamma and delta not.

mod support;

use std::path::PathBuf;
use std::sync::Mutex;

use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use agent_tui::screen::Screen;
use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;
use attend_chat::signal::{Channel, Signal};
use attend_chat::tabs::Tab;

const ALPHA_SID: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const GAMMA_SID: &str = "cccccccc-0000-4000-8000-000000000003";
const DELTA_SID: &str = "dddddddd-0000-4000-8000-000000000004";
/// Quiet for an hour: no fresh heartbeat, but within the hold a session no
/// record here names keeps (`attend_presence::UNJUDGED_HOLD`).
const EPSILON_SID: &str = "eeeeeeee-0000-4000-8000-000000000005";
const BATCH: &str = "001784732645123-4242";
const W: u16 = 120;
const H: u16 = 35;

/// One test at a time: they share the fake's log.
static SERIAL: Mutex<()> = Mutex::new(());

/// The fake attend: each call's argv a line of the log; a dry run reports
/// twelve to mark and three kept by two sessions; a delete marks twelve in
/// batch [`BATCH`]; an undo restores them; a delete in `#legacy` is refused
/// as an attend without the verb refuses it; a tombstone of delta is refused
/// as attend refuses one that has not exited. `#slow` answers in a second,
/// and a delete in `#long` (not its dry run) in eleven, past the ten the
/// chat once gave attend.
const FAKE: &str = r##"#!/bin/sh
echo "$*" >> "$FAKE_ATTEND_LOG"
case "$*" in
  *"--channel legacy"*) echo "error: unrecognized subcommand 'delete'" >&2; exit 2 ;;
  *"--channel slow"*) sleep 1; echo '{"scope":"#slow","dry_run":true,"marked":3,"batch":null,"ids":[],"held":[]}' ;;
  *"--channel long"*"--dry-run"*) echo '{"scope":"#long","dry_run":true,"marked":3,"batch":null,"ids":[],"held":[]}' ;;
  *"--channel long"*) sleep 11; echo '{"scope":"#long","dry_run":false,"marked":3,"batch":"001784732645123-4243","ids":[],"held":[]}' ;;
  *"--undo"*) echo '{"batch":"001784732645123-4242","age_secs":5,"unmarked":12,"already_removed":0}' ;;
  *"--dry-run"*) echo '{"scope":"#open","dry_run":true,"marked":12,"batch":null,"ids":[],"held":[{"session":"a","name":"Ada-x","kept":2},{"session":"b","name":"Bo-y","kept":1}]}' ;;
  "message delete"*) echo '{"scope":"#open","dry_run":false,"marked":12,"batch":"001784732645123-4242","ids":[],"held":[{"session":"a","name":"Ada-x","kept":2},{"session":"b","name":"Bo-y","kept":1}]}' ;;
  *"tombstone dddddddd"*) echo '{"error":{"kind":"not_exited","session":"x"},"message":"dddddddd-0000-4000-8000-000000000004 has not exited"}'; echo refused >&2; exit 1 ;;
  "message tombstone"*) echo '{"session":"x","tombstoned":true,"moved":false,"at":"2026-10-13T00:00:00Z"}' ;;
  *) echo "error: unexpected call" >&2; exit 3 ;;
esac
"##;

fn log_path() -> PathBuf {
    PathBuf::from(std::env::var_os("FAKE_ATTEND_LOG").expect("set by the fixture"))
}

fn fixture() -> std::sync::MutexGuard<'static, ()> {
    let guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let root = support::fixture("cleanup", |base| {
        support::pin_alive(ALPHA_SID);
        attend_presence::heartbeat::touch(EPSILON_SID).unwrap();
        let beat = std::fs::File::options().write(true).open(attend_presence::heartbeat::heartbeat_path(EPSILON_SID)).unwrap();
        beat.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600)).unwrap();
        std::fs::create_dir_all(base.join("@deploy")).unwrap();
        std::fs::create_dir_all(base.join("@legacy")).unwrap();
        std::fs::create_dir_all(base.join("@slow")).unwrap();
        std::fs::create_dir_all(base.join("@long")).unwrap();
    });
    let bin = root.join("bin");
    if !bin.join("attend").exists() {
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("attend"), FAKE).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(bin.join("attend"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{}:{path}", bin.display()));
        std::env::set_var("FAKE_ATTEND_LOG", root.join("attend.log"));
    }
    let _ = std::fs::remove_file(log_path());
    guard
}

/// Each call the fake took, as its argv.
fn calls() -> Vec<String> {
    std::fs::read_to_string(log_path()).unwrap_or_default().lines().map(str::to_string).collect()
}

fn sig(id: &str, from: &str, cwd: &str, message: &str, channel: Channel, ts: u64) -> Signal {
    Signal { id: id.into(), from: from.into(), project: cwd.rsplit('/').next().unwrap().into(), cwd: cwd.into(), reply_to: None, message: message.into(), ts, channel, tray: String::new(), beside_post: false }
}

/// A chat that runs its commands, its watermarks in `hidden`, its feed
/// alpha's, gamma's and delta's posts and a DM from alpha.
fn chat_in(hidden: &std::path::Path) -> Chat {
    let mut c = Chat::new(None, Palette::default(), Shape::PLAIN).sidebar(true).heartbeat(false).enhanced(true).hidden_file(hidden);
    for (i, (sid, cwd)) in [(ALPHA_SID, "/work/alpha"), (GAMMA_SID, "/work/gamma"), (DELTA_SID, "/work/delta"), (EPSILON_SID, "/work/epsilon")].into_iter().enumerate() {
        c.push(sig(&format!("o-{i}"), &format!("claude:{sid}"), cwd, &format!("{} on open", &cwd[6..]), Channel::Open, 100 + i as u64));
    }
    c.push(sig("d-1", &format!("claude:{ALPHA_SID}"), "/work/alpha", "alpha in deploy", Channel::Group("deploy".into()), 110));
    c.push(sig("p-1", &format!("claude:{ALPHA_SID}"), "/work/alpha", "alpha to you", Channel::Person, 120));
    c
}

fn chat() -> Chat {
    chat_in(&support_dir().join("hidden"))
}

/// A fresh directory for one test's watermarks, under the fixture, which
/// goes when the run ends.
fn support_dir() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let root = log_path().parent().expect("the fixture root").to_path_buf();
    root.join(format!("hidden-{}", NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)))
}

fn rows(c: &mut Chat) -> Vec<String> {
    testkit::rows(&testkit::render_screen(c, W, H))
}

fn screen(c: &mut Chat) -> String {
    rows(c).join("\n")
}

/// The feed and the compose box: each row right of the column's rule.
fn feed(c: &mut Chat) -> String {
    rows(c).iter().skip(1).map(|r| r.split_once('│').map_or(r.as_str(), |(_, rest)| rest).to_string()).collect::<Vec<_>>().join("\n")
}

fn press(c: &mut Chat, code: KeyCode) {
    press_now(c, code);
    answered(c);
}

/// [`press`] without waiting for attend's answer.
fn press_now(c: &mut Chat, code: KeyCode) {
    testkit::render_screen(c, W, H);
    testkit::drive(c, &[KeyEvent::new(code, KeyModifiers::NONE)]);
}

/// Tick the chat, as its loop does, until attend has answered what it was
/// asked, if anything.
fn answered(c: &mut Chat) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while c.removing().is_some() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
        Screen::tick(c);
    }
}

fn typed(c: &mut Chat, s: &str) {
    for ch in s.chars() {
        press(c, KeyCode::Char(ch));
    }
}

fn mouse(c: &mut Chat, button: MouseButton, x: u16, y: u16) {
    c.mouse(MouseEvent { kind: MouseEventKind::Down(button), column: x, row: y, modifiers: KeyModifiers::NONE });
}

/// A click with `button` on the first `text` in the left column.
fn side(c: &mut Chat, text: &str, button: MouseButton) {
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
    mouse(c, button, x, y);
}

/// The items of the open menu, inside its box.
fn items(c: &mut Chat) -> Vec<String> {
    let rows = rows(c);
    let Some(y) = rows.iter().rposition(|r| r.contains("pick one: ")) else { return Vec::new() };
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
    mouse(c, MouseButton::Left, x, y);
    answered(c);
}

/// [`pick`] without waiting for attend's answer.
fn pick_now(c: &mut Chat, label: &str) {
    let buf = testkit::render_screen(c, W, H);
    let (x, y) = testkit::find(&buf, label).unwrap_or_else(|| panic!("no {label}: {}", testkit::rows(&buf).join("\n")));
    mouse(c, MouseButton::Left, x, y);
}

/// A channel's menu, by a right click on its row.
fn channel_menu(c: &mut Chat, channel: &str) {
    side(c, channel, MouseButton::Right);
    assert!(screen(c).contains(&format!("pick one: {channel}")), "{}", screen(c));
}

fn nick(cwd: &str) -> String {
    agent_identity::Identity::for_cwd(cwd, agent_theme::ColorDepth::NoColor).nickname.to_string()
}

#[test]
fn delete_on_open_asks_with_the_dry_runs_counts_then_deletes_and_offers_undo() {
    let _g = fixture();
    let mut c = chat();
    channel_menu(&mut c, "#open");
    assert_eq!(items(&mut c), ["Hide messages here", "Delete read messages for everyone…"]);
    pick(&mut c, "Delete read messages");
    assert_eq!(items(&mut c), ["Current sessions", "Today (local midnight)", "All", "Custom…"], "{}", screen(&mut c));
    assert!(calls().is_empty(), "nothing run before the age is picked");
    pick(&mut c, "Today");
    assert_eq!(calls(), ["message delete --open --older-than today --dry-run --json"]);
    let s = screen(&mut c);
    assert!(s.contains("delete 12 read messages in #open from before today? · 3 stay unread by 2 sessions"), "{s}");
    press(&mut c, KeyCode::Char('n'));
    assert_eq!(c.status(), "kept");
    assert_eq!(calls().len(), 1, "no runs nothing");
    channel_menu(&mut c, "#open");
    pick(&mut c, "Delete read messages");
    pick(&mut c, "Today");
    press(&mut c, KeyCode::Char('y'));
    assert_eq!(calls().last().unwrap(), "message delete --open --older-than today --json");
    assert!(c.status().starts_with("deleted 12 messages in #open · 3 stay unread by 2 sessions") && c.status().ends_with(&format!("· Undo delete on its menu · batch {BATCH}")), "{}", c.status());
    channel_menu(&mut c, "#open");
    assert_eq!(items(&mut c), ["Hide messages here", "Delete read messages for everyone…", "Undo delete"]);
    pick(&mut c, "Undo delete");
    assert_eq!(calls().last().unwrap(), &format!("message delete --undo {BATCH} --json"));
    assert_eq!(c.status(), format!("restored 12 of 12 message(s) from batch {BATCH}"));
    channel_menu(&mut c, "#open");
    assert!(!items(&mut c).iter().any(|i| i == "Undo delete"), "undone once");
}

#[test]
fn each_scope_and_age_builds_its_argv() {
    let _g = fixture();
    let mut c = chat();
    channel_menu(&mut c, "#deploy");
    pick(&mut c, "Delete read messages");
    pick(&mut c, "All");
    press(&mut c, KeyCode::Esc);
    // A claude's row: the operator's DMs with it, by its session id.
    side(&mut c, &nick("/work/alpha"), MouseButton::Right);
    pick(&mut c, "Delete read messages");
    pick(&mut c, "Custom…");
    typed(&mut c, "soon");
    press(&mut c, KeyCode::Enter);
    assert!(screen(&mut c).contains("not a duration or a date YYYY-MM-DD"), "the dialog says what it takes: {}", screen(&mut c));
    for _ in 0..4 {
        press(&mut c, KeyCode::Backspace);
    }
    typed(&mut c, "3d");
    press(&mut c, KeyCode::Enter);
    assert!(screen(&mut c).contains("in your DMs with @") && screen(&mut c).contains("older than 3d?"), "{}", screen(&mut c));
    press(&mut c, KeyCode::Esc);
    // The DM view's row: the same thread.
    side(&mut c, &nick("/work/alpha"), MouseButton::Left);
    assert_eq!(c.foreground(), &Tab::Dm(ALPHA_SID.into()));
    side(&mut c, "DMs", MouseButton::Right);
    assert_eq!(items(&mut c), ["Hide DMs", "Hide messages here", "Delete read messages for everyone…"], "{}", screen(&mut c));
    pick(&mut c, "Delete read messages");
    pick(&mut c, "Current sessions");
    assert_eq!(
        calls(),
        [
            "message delete --channel deploy --older-than all --dry-run --json".to_string(),
            format!("message delete --dm {ALPHA_SID} --as-person --older-than 3d --dry-run --json"),
            format!("message delete --dm {ALPHA_SID} --as-person --older-than current --dry-run --json"),
        ]
    );
}

#[test]
fn purge_is_an_alias_of_delete_all_on_the_channel_shown() {
    let _g = fixture();
    let mut c = chat();
    side(&mut c, "#deploy", MouseButton::Left);
    // The keyboard back to the compose box, by a click on it.
    mouse(&mut c, MouseButton::Left, W - 5, H - 4);
    typed(&mut c, "/purge");
    press(&mut c, KeyCode::Enter);
    let s = screen(&mut c);
    assert_eq!(c.foreground(), &Tab::Channel("deploy".into()), "{s} {}", c.status());
    assert_eq!(calls(), ["message delete --channel deploy --older-than all --dry-run --json"], "{} | {}", c.status(), c.input().text());
    assert!(screen(&mut c).contains("delete 12 read messages in #deploy? · 3 stay"), "{}", screen(&mut c));
}

#[test]
fn an_attend_without_the_verb_says_it_needs_updating() {
    let _g = fixture();
    let mut c = chat();
    channel_menu(&mut c, "#legacy");
    pick(&mut c, "Delete read messages");
    pick(&mut c, "All");
    assert_eq!(c.status(), "`attend message delete` is missing: needs a newer attend");
    assert!(!screen(&mut c).contains("delete 12"), "nothing asked");
}

#[test]
fn tombstone_is_offered_on_a_dead_agents_row_only_and_shows_a_refusal() {
    let _g = fixture();
    let mut c = chat();
    side(&mut c, &nick("/work/alpha"), MouseButton::Right);
    assert!(!items(&mut c).iter().any(|i| i == "Tombstone…"), "a live agent: {:?}", items(&mut c));
    press(&mut c, KeyCode::Esc);
    side(&mut c, &nick("/work/epsilon"), MouseButton::Right);
    assert!(!items(&mut c).iter().any(|i| i == "Tombstone…"), "quiet, but attend would refuse it: {:?}", items(&mut c));
    press(&mut c, KeyCode::Esc);
    let gamma = nick("/work/gamma");
    side(&mut c, &gamma, MouseButton::Right);
    assert_eq!(items(&mut c).last().map(String::as_str), Some("Tombstone…"), "{}", screen(&mut c));
    pick(&mut c, "Tombstone…");
    assert!(screen(&mut c).contains(&format!("tombstone @{gamma}? its messages leave every tab")), "{}", screen(&mut c));
    assert!(calls().is_empty(), "asked first");
    press(&mut c, KeyCode::Char('y'));
    assert_eq!(calls(), [format!("message tombstone {GAMMA_SID} --json")]);
    assert!(c.status().starts_with(&format!("tombstoned @{gamma}")), "{}", c.status());
    side(&mut c, &nick("/work/delta"), MouseButton::Right);
    pick(&mut c, "Tombstone…");
    press(&mut c, KeyCode::Char('y'));
    assert_eq!(c.status(), format!("tombstone: {DELTA_SID} has not exited"), "attend's own words, after the verb");
}

#[test]
fn an_agents_own_dm_view_offers_tombstone_only_when_the_agent_is_dead() {
    let _g = fixture();
    let mut c = chat();
    for (cwd, want) in [("/work/alpha", vec!["Hide review"]), ("/work/delta", vec!["Hide review", "Tombstone…"])] {
        let name = nick(cwd);
        side(&mut c, &name, MouseButton::Right);
        pick(&mut c, "Review DMs");
        assert_eq!(c.foreground(), &Tab::Review(if cwd.ends_with("alpha") { ALPHA_SID.into() } else { DELTA_SID.into() }));
        side(&mut c, &format!("{name} DMs"), MouseButton::Right);
        assert_eq!(items(&mut c), want, "{}", screen(&mut c));
        pick(&mut c, "Hide review");
    }
}

#[test]
fn hide_messages_here_hides_in_its_tab_only_and_holds_across_a_restart() {
    let _g = fixture();
    let file = support_dir().join("hidden");
    let mut c = chat_in(&file);
    side(&mut c, "#open", MouseButton::Left);
    assert!(feed(&mut c).contains("alpha on open"), "{}", feed(&mut c));
    channel_menu(&mut c, "#open");
    pick(&mut c, "Hide messages here");
    assert!(screen(&mut c).contains("hide the messages in #open? the bus keeps every message"), "{}", screen(&mut c));
    press(&mut c, KeyCode::Char('y'));
    assert_eq!(c.status(), "hid 5 messages in #open; the bus keeps them");
    assert!(!feed(&mut c).contains("on open"), "{}", feed(&mut c));
    assert!(calls().is_empty(), "nothing on the bus");
    c.push(sig("o-9", &format!("claude:{ALPHA_SID}"), "/work/alpha", "alpha later", Channel::Open, 200));
    assert!(feed(&mut c).contains("alpha later"), "a newer message shows");
    let buf = testkit::render_screen(&mut c, W, H);
    let (x, y) = testkit::find(&buf, "⋯all").expect("the operator view's tab");
    mouse(&mut c, MouseButton::Left, x, y);
    assert_eq!(c.foreground(), &Tab::Merged);
    let all = |c: &mut Chat| rows(c).iter().skip(1).map(|r| r.split_once('│').map_or("", |(_, f)| f).to_string()).collect::<String>();
    assert!(all(&mut c).contains("alpha on open"), "the operator view keeps them: {}", screen(&mut c));
    // A new chat on the same state file: #open still hides them.
    let mut again = chat_in(&file);
    side(&mut again, "#open", MouseButton::Left);
    assert!(!feed(&mut again).contains("on open"), "{}", feed(&mut again));
    assert!(std::fs::read_to_string(&file).unwrap().starts_with("#open\t"), "kept in the state file");
    // Show hidden messages takes the mark away, there and in the file.
    channel_menu(&mut again, "#open");
    assert_eq!(items(&mut again), ["Hide messages here", "Show hidden messages", "Delete read messages for everyone…"]);
    pick(&mut again, "Show hidden messages");
    assert_eq!(again.status(), "#open shows every message again");
    assert!(feed(&mut again).contains("alpha on open"), "{}", feed(&mut again));
    assert!(!std::fs::read_to_string(&file).unwrap().contains("#open\t"), "gone from the state file");
    channel_menu(&mut again, "#open");
    assert!(!items(&mut again).iter().any(|i| i == "Show hidden messages"), "nothing hidden: nothing to show");
}

/// A named channel offers Hide messages here as `#open` does, and Show
/// hidden messages while it has a mark; the mark is the channel's alone.
#[test]
fn a_named_channel_hides_its_messages_here_too() {
    let _g = fixture();
    let mut c = chat();
    side(&mut c, "#deploy", MouseButton::Left);
    assert!(feed(&mut c).contains("alpha in deploy"), "{}", feed(&mut c));
    channel_menu(&mut c, "#deploy");
    let menu = items(&mut c);
    assert_eq!(menu.iter().position(|i| i == "Hide messages here"), Some(3), "after Describe…, before the delete: {menu:?}");
    pick(&mut c, "Hide messages here");
    assert!(screen(&mut c).contains("hide the messages in #deploy? the bus keeps every message"), "{}", screen(&mut c));
    press(&mut c, KeyCode::Char('y'));
    assert!(c.status().starts_with("hid ") && c.status().ends_with(" in #deploy; the bus keeps them"), "{}", c.status());
    assert!(!feed(&mut c).contains("alpha in deploy"), "{}", feed(&mut c));
    assert!(calls().is_empty(), "nothing on the bus");
    side(&mut c, "#open", MouseButton::Left);
    assert!(feed(&mut c).contains("alpha on open"), "#open keeps its messages: {}", feed(&mut c));
    channel_menu(&mut c, "#deploy");
    assert!(items(&mut c).iter().any(|i| i == "Show hidden messages"), "{:?}", items(&mut c));
    pick(&mut c, "Show hidden messages");
    assert_eq!(c.status(), "#deploy shows every message again");
    side(&mut c, "#deploy", MouseButton::Left);
    assert!(feed(&mut c).contains("alpha in deploy"), "{}", feed(&mut c));
}

/// The mark is the newest bus message's, never a notice of the chat's own,
/// which is timed by the clock now; and `/clear` asks, as the item does.
#[test]
fn a_notice_does_not_carry_the_mark_and_clear_asks_first() {
    let _g = fixture();
    let mut c = chat();
    side(&mut c, "#open", MouseButton::Left);
    c.push(attend_chat::signal::compose_status_block("peers", "a notice of the chat's"));
    mouse(&mut c, MouseButton::Left, W - 5, H - 4);
    typed(&mut c, "/clear");
    press(&mut c, KeyCode::Enter);
    assert!(screen(&mut c).contains("hide the messages in #open? the bus keeps every message"), "asks first: {}", screen(&mut c));
    assert!(feed(&mut c).contains("alpha on open"), "nothing hidden before the y");
    press(&mut c, KeyCode::Char('y'));
    assert_eq!(c.status(), "hid 5 messages in #open; the bus keeps them", "the notice is not counted");
    c.push(sig("o-late", &format!("claude:{ALPHA_SID}"), "/work/alpha", "sent before the notice, arrived after the hide", Channel::Open, 300));
    assert!(feed(&mut c).contains("arrived after the hide"), "newer than the last bus message, so shown: {}", feed(&mut c));
}

#[test]
fn attend_answers_off_the_screens_thread_one_request_at_a_time() {
    let _g = fixture();
    let mut c = chat();
    channel_menu(&mut c, "#slow");
    pick(&mut c, "Delete read messages");
    pick_now(&mut c, "All");
    assert_eq!(c.status(), "counting read messages…", "the pick returns at once");
    channel_menu(&mut c, "#open");
    pick_now(&mut c, "Delete read messages");
    assert_eq!(c.status(), "still counting read messages: one delete or tombstone runs at a time");
    assert!(!items(&mut c).iter().any(|i| i == "All"), "refused before its ages: {}", screen(&mut c));
    assert_eq!(calls(), ["message delete --channel slow --older-than all --dry-run --json"], "the second pick ran nothing");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !screen(&mut c).contains("delete 3 read messages in #slow?") && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
        Screen::tick(&mut c);
    }
    assert!(screen(&mut c).contains("delete 3 read messages in #slow?"), "the answer asks on a tick: {}", screen(&mut c));
}

/// A delete runs to its end however long attend takes, past the ten
/// seconds the chat once gave it. Until then the status line says
/// `deleting…`, a fresh note taking the line for its few seconds; the
/// screen stays live; and another delete or a tombstone asked for
/// meanwhile is refused with a note.
#[test]
fn a_long_delete_runs_to_its_end_and_says_deleting_until_then() {
    use std::time::{Duration, Instant};
    let _g = fixture();
    let mut c = chat();
    channel_menu(&mut c, "#long");
    pick(&mut c, "Delete read messages");
    pick(&mut c, "All");
    assert!(screen(&mut c).contains("delete 3 read messages in #long?"), "{}", screen(&mut c));
    let began = Instant::now();
    press_now(&mut c, KeyCode::Char('y'));
    assert_eq!(c.status(), "deleting…", "the y returns at once");
    assert_eq!(c.removing().as_deref(), Some("deleting…"));
    c.push(sig("o-new", &format!("claude:{ALPHA_SID}"), "/work/alpha", "news while deleting", Channel::Open, 400));
    side(&mut c, "#open", MouseButton::Left);
    assert!(feed(&mut c).contains("news while deleting"), "the screen stays live: {}", feed(&mut c));
    channel_menu(&mut c, "#open");
    pick_now(&mut c, "Delete read messages");
    assert_eq!(c.status(), "still deleting: one delete or tombstone runs at a time");
    assert!(!items(&mut c).iter().any(|i| i == "All"), "refused before its ages: {}", screen(&mut c));
    side(&mut c, &nick("/work/gamma"), MouseButton::Right);
    pick_now(&mut c, "Tombstone…");
    assert_eq!(c.status(), "still deleting: one delete or tombstone runs at a time");
    assert!(!screen(&mut c).contains("tombstone @") && !screen(&mut c).contains("pick one: "), "nothing asked, nothing open: {}", screen(&mut c));
    // Past the note's few seconds the line says deleting… again.
    let status_row = |c: &mut Chat| rows(c).last().cloned().unwrap_or_default();
    while !status_row(&mut c).contains("deleting…") && began.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(50));
        Screen::tick(&mut c);
    }
    assert!(status_row(&mut c).contains("deleting…") && c.removing().is_some(), "{:?} after {:?}", status_row(&mut c), began.elapsed());
    while c.removing().is_some() && began.elapsed() < Duration::from_secs(30) {
        std::thread::sleep(Duration::from_millis(50));
        Screen::tick(&mut c);
    }
    assert!(began.elapsed() >= Duration::from_secs(10), "attend ran past ten seconds: {:?}", began.elapsed());
    assert!(c.status().starts_with("deleted 3 messages in #long"), "{}", c.status());
    assert_eq!(calls(), ["message delete --channel long --older-than all --dry-run --json", "message delete --channel long --older-than all --json"], "the refused requests ran nothing");
}
