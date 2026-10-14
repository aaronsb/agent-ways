//! The operator's direct messages read back from every tray (issue #995):
//! a DM the operator sent to an agent in another project is on disk in that
//! project's tray, and the chat reads it from there, whatever session sent
//! it and whatever else the same message addressed. A DM between two other
//! agents is read too, and no view of the operator's shows it. Through the
//! real watcher and the shell's real key and mouse handlers. In its own
//! file, so its own fixture: alpha and beta live, in projects that are not
//! the chat's.

mod support;

use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;
use attend_chat::signal::{self, Channel, Signal};
use attend_chat::tabs::{self, Tab};
use attend_chat::watcher;

const ALPHA_SID: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const BETA_SID: &str = "bbbbbbbb-0000-4000-8000-000000000002";
const ALPHA_CWD: &str = "/work/alpha";
const BETA_CWD: &str = "/work/beta";
const W: u16 = 120;
const H: u16 = 35;

/// What the operator sent alpha in an earlier session, on disk before any
/// chat in this run starts.
const EARLIER: &str = "sent alpha from an earlier session";
/// What beta sent alpha, in alpha's tray: a DM between others.
const ASIDE: &str = "beta to alpha, not the operator's";

fn base() -> &'static Path {
    static BASE: OnceLock<&'static Path> = OnceLock::new();
    BASE.get_or_init(|| {
        support::fixture("dm-trays", |base| {
            support::pin_alive(ALPHA_SID);
            support::pin_alive(BETA_SID);
            let sessions = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join(".claude").join("sessions");
            std::fs::create_dir_all(&sessions).unwrap();
            for (pid, sid, cwd) in [(4241, ALPHA_SID, ALPHA_CWD), (4242, BETA_SID, BETA_CWD)] {
                std::fs::write(sessions.join(format!("{pid}.json")), format!(r#"{{"pid":{pid},"sessionId":"{sid}","cwd":"{cwd}"}}"#)).unwrap();
            }
            attend_groups::Groups::new(base, ALPHA_SID).join("deploy", true).unwrap();
            signal::write_signal(&signal::cwd_dir(ALPHA_CWD), EARLIER, ALPHA_SID).unwrap();
            let line = agent_identity::signal::format_tagged(
                &format!("claude:{BETA_SID}"),
                "beta",
                BETA_CWD,
                &agent_identity::Envelope { from_kind: Some("agent"), from_id: Some(BETA_SID), on_behalf_of: None, to: Some(ALPHA_SID) },
                None,
                ASIDE,
            );
            agent_identity::signal::write_signal_file(&signal::cwd_dir(ALPHA_CWD), "beta-aside-1-0.signal", &line).unwrap();
        });
        Box::leak(signal::signals_base().into_boxed_path())
    })
}

/// A chat fed by a watcher of its own over the fixture's signals base, as
/// `main` starts it: the chat's project is this test's working directory,
/// which neither agent works in. Enter writes.
fn live_chat() -> Chat {
    let base = base();
    let (tx, rx) = std::sync::mpsc::channel::<Signal>();
    let here = std::env::current_dir().unwrap().to_string_lossy().to_string();
    watcher::spawn_watcher(base.to_path_buf(), signal::encode_cwd(&here), signal::human_member_id(), tx).unwrap();
    Chat::new(Some(rx), Palette::default(), Shape::PLAIN).sidebar(true).heartbeat(false).enhanced(true)
}

/// Drain the watcher until a message holding `body` is in the chat, or
/// five seconds pass; then a moment more, so a second copy would be in too.
fn arrived(c: &mut Chat, body: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && !c.signals().iter().any(|s| s.message.contains(body)) {
        c.drain();
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(300));
    c.drain();
    assert!(c.signals().iter().any(|s| s.message.contains(body)), "{body} never arrived: {:#?}", c.signals());
}

fn rows(c: &mut Chat) -> Vec<String> {
    testkit::rows(&testkit::render_screen(c, W, H))
}

/// The feed and the compose box: each row right of the column's rule.
fn feed(c: &mut Chat) -> String {
    rows(c).iter().skip(1).map(|r| r.split_once('│').map_or(r.as_str(), |(_, rest)| rest).to_string()).collect::<Vec<_>>().join("\n")
}

fn press(c: &mut Chat, code: KeyCode) {
    testkit::render_screen(c, W, H);
    testkit::drive(c, &[KeyEvent::new(code, KeyModifiers::NONE)]);
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

fn nick(cwd: &str) -> String {
    agent_identity::Identity::for_cwd(cwd, agent_theme::ColorDepth::NoColor).nickname.to_string()
}

/// How many messages `tab` shows that hold `body`.
fn shown_in(c: &Chat, tab: &Tab, body: &str) -> usize {
    c.signals().iter().filter(|s| s.message.contains(body) && tabs::shows(s, tab)).count()
}

/// Gap 5 of ADR-405's amendment: a DM the operator sent another project's
/// agent in an earlier session is read back from that agent's tray, as
/// sent, to whom, and listed in the agent's DM view.
#[test]
fn a_dm_sent_in_an_earlier_session_is_read_back_into_the_agents_view() {
    let mut c = live_chat();
    arrived(&mut c, EARLIER);
    let sent = c.signals().iter().find(|s| s.message == EARLIER).unwrap();
    assert_eq!(sent.channel, Channel::Sent(ALPHA_SID.into()), "what the operator sent, to whom");
    side_click(&mut c, "alpha");
    assert_eq!(c.foreground(), &Tab::Dm(ALPHA_SID.into()));
    let f = feed(&mut c);
    assert_eq!(f.matches(EARLIER).count(), 1, "listed once in alpha's view: {f}");
    assert!(f.contains(&format!("→ @{}", nick(ALPHA_CWD))), "labelled with its recipient: {f}");
}

/// Review of #993, finding 2: `@Name #channel` from the operator view goes
/// to the channel and, as a DM, to the agent's tray in another project.
/// The DM is read back from that tray into the agent's view, once.
#[test]
fn a_dm_sent_beside_a_channel_post_reaches_the_agents_view() {
    let mut c = live_chat();
    arrived(&mut c, EARLIER);
    let body = "hi from beside the deploy post";
    typed(&mut c, &format!("@{} #deploy {body}", nick(ALPHA_CWD)));
    press(&mut c, KeyCode::Enter);
    assert!(c.input().is_empty(), "sent: {}", c.status());
    arrived(&mut c, body);
    assert_eq!(shown_in(&c, &Tab::Dm(ALPHA_SID.into()), body), 1, "in alpha's view once: {:#?}", c.signals());
    assert_eq!(shown_in(&c, &Tab::Channel("deploy".into()), body), 1, "in #deploy once");
}

/// One message addressed to two agents in two projects is written to both
/// trays and read back from both: each agent's view lists it once.
#[test]
fn a_dm_to_two_projects_is_listed_once_in_each_agents_view() {
    let mut c = live_chat();
    arrived(&mut c, EARLIER);
    let body = "one message for alpha and beta";
    typed(&mut c, &format!("@{} @{} {body}", nick(ALPHA_CWD), nick(BETA_CWD)));
    press(&mut c, KeyCode::Enter);
    assert!(c.input().is_empty(), "sent: {}", c.status());
    arrived(&mut c, body);
    for sid in [ALPHA_SID, BETA_SID] {
        assert_eq!(shown_in(&c, &Tab::Dm(sid.into()), body), 1, "{sid}: {:#?}", c.signals());
    }
    assert_eq!(shown_in(&c, &Tab::Merged, body), 1, "the operator view lists it once");
}

/// A DM between two other agents, in a tray that is not the chat's, is read
/// but no view of the operator's shows it: not the operator view, not a
/// channel, not either agent's DMs with the operator.
#[test]
fn a_dm_between_others_shows_in_no_view_of_the_operators() {
    let mut c = live_chat();
    arrived(&mut c, ASIDE);
    for tab in [Tab::Merged, Tab::Channel("open".into()), Tab::Dm(ALPHA_SID.into()), Tab::Dm(BETA_SID.into())] {
        assert_eq!(shown_in(&c, &tab, ASIDE), 0, "{tab:?}");
    }
    assert!(!feed(&mut c).contains(ASIDE), "{}", feed(&mut c));
}
