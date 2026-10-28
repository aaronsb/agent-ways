//! A differential trace of the chat's interaction (ADR-407, amendment of
//! 2026-10-12): seeded random keys, clicks, right clicks, the wheel,
//! pastes, and messages arriving through the watcher, driven through the
//! shell's real handlers. Each step records the tab shown, the draft and
//! its cursor, the status, and most steps the whole frame.
//!
//! A refactor that changes no behaviour leaves the trace byte-identical.
//! Each frame recorded carries a digest of its cells' styles, so the caret,
//! a focused border and the focus marks count as well as the text. CI holds
//! a digest of a small fixed run ([`PINNED`]); a change in behaviour changes
//! it, and re-pinning is a reviewed change. To see what moved, record the
//! full trace on the base and on the change and compare the two files:
//!
//! ```text
//! IX_TRACE=/path/before.txt cargo test -p attend-chat --test ix_trace -- --ignored record
//! IX_TRACE=/path/after.txt  cargo test -p attend-chat --test ix_trace -- --ignored record
//! cmp /path/before.txt /path/after.txt
//! ```
//!
//! `IX_SEEDS` and `IX_STEPS` set the size of the run (60 seeds of 120
//! steps by default). A message from a sender the chat has not seen reads
//! the world again on the next frame, which refocuses: the trace reaches
//! the refresh path as well as the keys. In its own file, so its own
//! fixture: alpha and beta live, alpha and the operator in #deploy, an
//! empty #infra.

mod support;

use std::fmt::Write;
use std::sync::mpsc::{channel, Sender};

use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use agent_tui::screen::Screen;
use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;
use attend_chat::signal::{Channel, Signal};

const ALPHA_SID: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const BETA_SID: &str = "bbbbbbbb-0000-4000-8000-000000000002";
const NOW: u64 = 1_790_950_000;

fn signal(i: usize, sid: &str, cwd: &str, channel: Channel) -> Signal {
    Signal {
        id: format!("t-{i}"),
        from: format!("claude:{sid}"),
        project: cwd.rsplit('/').next().unwrap().into(),
        cwd: cwd.into(),
        reply_to: None,
        message: format!("message {i} in the feed"),
        ts: NOW - 1000 + i as u64,
        channel,
        tray: String::new(),
        beside_post: false,
    }
}

/// The chat and the watcher's sending end. The feed holds three messages
/// from alpha and beta, then one from each of twenty others, so the
/// column's agents overflow it at either size and the wheel and the
/// cursor both have somewhere to scroll it.
fn chat() -> (Chat, Sender<attend_chat::watcher::Delivery>) {
    support::fixture("trace", |base| {
        support::pin_alive(ALPHA_SID);
        support::pin_alive(BETA_SID);
        let alpha = attend_groups::Groups::new(base, ALPHA_SID);
        alpha.create("deploy", Some("rollout")).unwrap();
        alpha.join("deploy", true).unwrap();
        attend_groups::Groups::new(base, &attend_chat::signal::human_member_id()).join("deploy", true).unwrap();
        attend_groups::Groups::new(base, "").create("infra", None).unwrap();
    });
    let (tx, rx) = channel();
    let mut c = Chat::new(Some(rx), Palette::default(), Shape::PLAIN)
        .sidebar(true)
        .heartbeat(false)
        .enhanced(true)
        .dry_run(true)
        // A question's yes arms at once: the arm runs on the clock, and the
        // trace holds no time.
        .question_arm(std::time::Duration::ZERO)
        .pinned(std::time::UNIX_EPOCH + std::time::Duration::from_secs(NOW), 0);
    for (i, (sid, cwd)) in [(ALPHA_SID, "/work/alpha"), (BETA_SID, "/work/beta"), (ALPHA_SID, "/work/alpha")].into_iter().enumerate() {
        c.push(signal(i, sid, cwd, Channel::Open));
    }
    for i in 0..20 {
        c.push(signal(3 + i, &format!("dddddddd-0000-4000-8000-{i:012}"), &format!("/work/old{i}"), Channel::Open));
    }
    (c, tx)
}

/// xorshift64: the same sequence from the same seed on every machine.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn k(code: KeyCode, m: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, m)
}

/// The keys a step picks from: the ring's, the compose box's, the shell's
/// single keys and chords, the column's toggle.
fn keys() -> Vec<KeyEvent> {
    let n = KeyModifiers::NONE;
    vec![
        k(KeyCode::Tab, n),
        k(KeyCode::BackTab, KeyModifiers::SHIFT),
        k(KeyCode::Left, n),
        k(KeyCode::Right, n),
        k(KeyCode::Up, n),
        k(KeyCode::Down, n),
        k(KeyCode::Up, n),
        k(KeyCode::Down, n),
        k(KeyCode::Home, n),
        k(KeyCode::End, n),
        k(KeyCode::Enter, n),
        k(KeyCode::Esc, n),
        k(KeyCode::Esc, n),
        k(KeyCode::PageUp, n),
        k(KeyCode::PageDown, n),
        k(KeyCode::Backspace, n),
        k(KeyCode::Delete, n),
        k(KeyCode::Char('a'), n),
        k(KeyCode::Char('x'), n),
        k(KeyCode::Char('#'), n),
        k(KeyCode::Char('q'), n),
        k(KeyCode::Char('?'), n),
        k(KeyCode::Char('y'), n),
        k(KeyCode::Char('D'), n),
        k(KeyCode::Char('1'), n),
        k(KeyCode::Char('2'), n),
        k(KeyCode::Char('0'), n),
        k(KeyCode::Char('3'), n),
        k(KeyCode::F(2), n),
        k(KeyCode::Char('t'), KeyModifiers::CONTROL),
        k(KeyCode::Char('1'), KeyModifiers::ALT),
        k(KeyCode::Char('2'), KeyModifiers::CONTROL),
        k(KeyCode::Char('n'), KeyModifiers::CONTROL),
        k(KeyCode::Char('c'), KeyModifiers::CONTROL),
        k(KeyCode::Enter, KeyModifiers::SHIFT),
        k(KeyCode::F(3), n),
        k(KeyCode::Char('b'), KeyModifiers::CONTROL),
        k(KeyCode::Char(' '), n),
        k(KeyCode::F(1), n),
        k(KeyCode::Char('m'), n),
    ]
}

/// The digest of twenty seeds of 200 steps ([`pinned_run`]).
const PINNED: &str = "0c46ae422330dd3d";

/// FNV-1a over `bytes`, from `h`: a digest stable on every machine.
fn fnv(h: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(h, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3))
}

const FNV_START: u64 = 0xcbf2_9ce4_8422_2325;

/// A digest of every cell's style in `buf`: colours and modifiers, the
/// caret's reversed cell included.
fn styles(buf: &agent_tui::ratatui::buffer::Buffer) -> u64 {
    buf.content.iter().fold(FNV_START, |h, c| fnv(h, format!("{:?}", c.style()).as_bytes()))
}

/// The trace of `seeds` runs of `steps` steps each.
fn trace(seeds: std::ops::RangeInclusive<u64>, steps: u64) -> String {
    let mut out = String::new();
    let keys = keys();
    let n = KeyModifiers::NONE;
    for seed in seeds {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let (mut c, tx) = chat();
        let (w, h) = if rng.below(2) == 0 { (120u16, 35u16) } else { (80, 25) };
        let mut sent = 23;
        writeln!(out, "== seed {seed} {w}x{h}").unwrap();
        for step in 0..steps {
            let roll = rng.below(100);
            let what = if roll < 62 {
                let key = keys[rng.below(keys.len() as u64) as usize];
                Screen::key(&mut c, key);
                format!("key {:?}{:?}", key.code, key.modifiers)
            } else if roll < 84 {
                let kind = if rng.below(4) == 0 { MouseEventKind::Down(MouseButton::Right) } else { MouseEventKind::Down(MouseButton::Left) };
                let row = if rng.below(3) == 0 { 0 } else { rng.below(h as u64) as u16 };
                let col = rng.below(w as u64) as u16;
                Screen::mouse(&mut c, MouseEvent { kind, column: col, row, modifiers: n });
                format!("click {kind:?} {col},{row}")
            } else if roll < 91 {
                let kind = if rng.below(2) == 0 { MouseEventKind::ScrollUp } else { MouseEventKind::ScrollDown };
                let row = rng.below(h as u64) as u16;
                let col = rng.below(w as u64) as u16;
                Screen::mouse(&mut c, MouseEvent { kind, column: col, row, modifiers: n });
                format!("wheel {kind:?} {col},{row}")
            } else if roll < 97 {
                // A message arrives: from a known sender, or from one the
                // chat has not seen, on #open or on #deploy.
                let fresh = rng.below(3) == 0;
                let (sid, cwd) = match (fresh, rng.below(2)) {
                    (true, _) => (format!("cccccccc-0000-4000-8000-{sent:012}"), format!("/work/new{sent}")),
                    (false, 0) => (ALPHA_SID.to_string(), "/work/alpha".to_string()),
                    (false, _) => (BETA_SID.to_string(), "/work/beta".to_string()),
                };
                let ch = if rng.below(2) == 0 { Channel::Open } else { Channel::Group("deploy".into()) };
                tx.send(signal(sent, &sid, &cwd, ch.clone()).into()).unwrap();
                sent += 1;
                c.drain();
                format!("signal {sid} {ch:?}")
            } else {
                Screen::paste(&mut c, "pasted");
                "paste".to_string()
            };
            let mut line = format!("{step} {what} | fg={:?} in={:?}@{} st={:?} side={}", c.foreground(), c.input().text(), c.input().cursor(), c.status(), c.sidebar_shown());
            if rng.below(4) != 0 {
                let buf = testkit::render_screen(&mut c, w, h);
                let text = testkit::rows(&buf).join("\n");
                let _ = write!(line, " styles={:016x}\n{text}", styles(&buf));
            }
            writeln!(out, "{line}").unwrap();
            if c.app().ended() {
                writeln!(out, "ended").unwrap();
                break;
            }
        }
    }
    out
}

/// The run CI pins: twenty seeds of 200 steps.
fn pinned_run() -> String {
    trace(1..=20, 200)
}

/// The chat's interaction is what it was when [`PINNED`] was recorded. A
/// change that means to change behaviour re-pins it, and the review reads
/// the trace's difference.
#[test]
fn the_pinned_trace_is_unchanged() {
    let run = pinned_run();
    assert!(run.contains(" signal ") && run.contains(" wheel ") && run.contains(" styles="), "the run reaches the watcher and the wheel, and records styles");
    let got = format!("{:016x}", fnv(FNV_START, run.as_bytes()));
    assert!(
        got == PINNED,
        "the trace of the chat's interaction changed: {PINNED} pinned, {got} now.\n\
         If the change in behaviour is intended, see what moved: record the full trace\n\
         (`IX_TRACE=<file> cargo test -p attend-chat --test ix_trace -- --ignored record`)\n\
         on the base and on this change and compare them. Then set PINNED in\n\
         tools/attend-chat/tests/ix_trace.rs to {got}, and say why in the PR."
    );
}

/// Record the trace to `IX_TRACE`, for comparing two revisions.
#[test]
#[ignore = "a differential tool: run it on two revisions and compare the files"]
fn record() {
    let path = std::env::var("IX_TRACE").expect("IX_TRACE names the file to write");
    let seeds: u64 = std::env::var("IX_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(60);
    let steps: u64 = std::env::var("IX_STEPS").ok().and_then(|s| s.parse().ok()).unwrap_or(120);
    std::fs::write(path, trace(1..=seeds, steps)).unwrap();
}
