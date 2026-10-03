//! What the shell adds over the spike's screens: the adapter does the real
//! work of an apply, a typed secret reaches only its command's stdin, a
//! locked value and a finding show and refuse, the tree reloads from the
//! adapter keeping what is pending, and the help overlay carries the
//! adapter's text.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;

use super::*;
use crate::adapter::Write;
use crate::testkit::{finish_apply, press, render, text, type_str};
use crate::tree::{Action, Kind, Node, Setting, Store};

/// An adapter whose every hook a test can see and steer.
#[derive(Default, Clone)]
struct Probe {
    written: Rc<RefCell<Vec<String>>>,
    stdin: Rc<RefCell<Vec<String>>>,
    fresh: Rc<RefCell<Option<Vec<Node>>>>,
    reloads: Rc<Cell<usize>>,
    stamp: Rc<Cell<u64>>,
    views: Rc<RefCell<Vec<String>>>,
    /// A view was switched: its files join the watch when the tree is next
    /// read, which changes the stamp.
    widened: Rc<Cell<bool>>,
    /// Each view switch's pending keys; a set refusal refuses the switch.
    seen_pending: Rc<RefCell<Vec<Vec<String>>>>,
    refuse: Rc<RefCell<Option<String>>>,
    /// The commands run, in order.
    ran: Rc<RefCell<Vec<String>>>,
}

impl Adapter for Probe {
    fn validate(&self, _: &Store, text: &str) -> Option<Result<String, String>> {
        Some(match text.trim().parse::<f64>() {
            Ok(v) if (0.0..=1.0).contains(&v) => Ok(format!("{v}")),
            _ => Err(format!("`{text}` is not a probability")),
        })
    }
    fn write(&mut self, file: &Path, values: &[Write]) -> Result<(), String> {
        let kv: Vec<String> = values.iter().map(|w| format!("{}={}", w.store.key, w.value)).collect();
        self.written.borrow_mut().push(format!("{} {}", file.display(), kv.join(" ")));
        Ok(())
    }
    fn run(&mut self, q: &Queued) -> Result<(), String> {
        self.ran.borrow_mut().push(q.command.clone());
        if q.command.contains("rejected") {
            return Err("the key was refused".into());
        }
        if let Some(s) = &q.stdin {
            self.stdin.borrow_mut().push(s.reveal().to_string());
        }
        Ok(())
    }
    fn reload(&mut self) -> Option<Vec<Node>> {
        self.reloads.set(self.reloads.get() + 1);
        if self.widened.replace(false) {
            self.stamp.set(self.stamp.get() + 1);
        }
        self.fresh.borrow().clone()
    }
    fn stamp(&self) -> Option<u64> {
        Some(self.stamp.get())
    }
    fn help(&self, tab: &str) -> Option<String> {
        Some(format!("{tab}: what this tab holds\n  a line of {tab} help\n"))
    }
    fn view(&mut self, name: &str, pending: &[&Store]) -> Result<Option<String>, String> {
        self.seen_pending.borrow_mut().push(pending.iter().map(|s| s.key.clone()).collect());
        if let Some(why) = self.refuse.borrow().clone() {
            return Err(why);
        }
        self.views.borrow_mut().push(name.to_string());
        self.widened.set(true);
        Ok(Some(format!("showing {name}")))
    }
    fn title(&self, tab: &str) -> Option<String> {
        Some(format!("{tab} · {} views", self.views.borrow().len()))
    }
}

fn prob(v: &str, file: &str, key: &str) -> Setting {
    Setting::new(Kind::Float { min: 0.0, max: 1.0 }, v, "user").default("0.5").store("user", file.into(), key)
}

/// One tab: two probabilities in one file, a key with a secret action.
fn tree() -> Vec<Node> {
    let key = Node::leaf("anthropic", "", Setting::new(Kind::Secret, "absent", "keys/"))
        .with_actions(vec![Action::new("set", "ways agent key add --provider anthropic").arg(Arg::Secret)]);
    vec![Node::group("matching", "", vec![Node::leaf("tau", "", prob("0.5", "/c/a.yaml", "matching.tau")), Node::leaf("floor", "", prob("0.2", "/c/a.yaml", "matching.floor")), key]).opened()]
}

fn probe() -> (App, Probe) {
    let p = Probe::default();
    (App::new("t", tree()).adapter(p.clone()), p)
}

/// An action that only reads runs at once: nothing is queued, there is no
/// review, the bottom bar says how it ended, and the tree is read again. An
/// action that changes things is still queued.
#[test]
fn a_reading_action_runs_at_once_without_the_queue() {
    let p = Probe::default();
    let actions = vec![Action::new("check", "probe check").reads(), Action::new("check rejected", "probe check rejected").reads(), Action::new("rotate", "probe rotate")];
    let roots = vec![Node::group("keys", "", vec![Node::leaf("anthropic", "", Setting::new(Kind::Text, "present", "computed"))]).with_actions(actions)];
    let mut app = App::new("t", roots).adapter(p.clone());
    app.pick(vec![0], 0);
    assert_eq!(*p.ran.borrow(), ["probe check"]);
    assert_eq!(app.queue.len(), 0, "nothing to review or apply");
    assert_eq!(app.msg, "check: ok");
    assert_eq!(p.reloads.get(), 1, "what it found may show in the tree");
    app.pick(vec![0], 1);
    assert_eq!(app.msg, "check rejected: the key was refused");
    app.pick(vec![0], 2);
    assert_eq!(app.queue.len(), 1, "a changing action is still queued");
    assert_eq!(p.ran.borrow().len(), 2);
}

#[test]
fn an_apply_writes_through_the_adapter_and_reloads_after() {
    let (mut app, p) = probe();
    press(&mut app, &[KeyCode::Char('e')]);
    app.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    type_str(&mut app, "0.40");
    press(&mut app, &[KeyCode::Enter, KeyCode::Down, KeyCode::Char('e')]);
    app.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    type_str(&mut app, "0.3");
    press(&mut app, &[KeyCode::Enter, KeyCode::Char('w'), KeyCode::Char('a')]);
    finish_apply(&mut app);
    assert_eq!(*p.written.borrow(), ["/c/a.yaml matching.tau=0.4 matching.floor=0.3"], "one write per file, the adapter's normal form");
    assert_eq!(p.reloads.get(), 1, "a write that landed reloads the tree");
    assert_eq!(app.pending(), 0);
}

#[test]
fn the_adapter_check_rejects_text_and_keeps_the_entry() {
    let (mut app, _) = probe();
    press(&mut app, &[KeyCode::Char('e')]);
    app.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    type_str(&mut app, "lots");
    press(&mut app, &[KeyCode::Enter]);
    assert!(app.message().contains("rejected: `lots` is not a probability"), "{}", app.message());
    assert!(matches!(app.mode, Mode::Edit(_)), "the entry stays open");
    assert_eq!(app.pending(), 0);
}

#[test]
fn a_typed_secret_reaches_only_its_commands_stdin() {
    const SECRET: &str = "sk-Q#Z9@X!";
    let (mut app, p) = probe();
    press(&mut app, &[KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
    type_str(&mut app, SECRET);
    press(&mut app, &[KeyCode::Enter]);
    assert!(!format!("{:?}", app.queued()).contains(SECRET), "the queue's Debug redacts it");
    press(&mut app, &[KeyCode::Char('w')]);
    assert!(!text(&render(&mut app, 100, 30)).contains(SECRET));
    press(&mut app, &[KeyCode::Char('a')]);
    finish_apply(&mut app);
    assert_eq!(*p.stdin.borrow(), [SECRET], "the command got it on stdin");
    assert!(app.queued().is_empty() && !app.summary().contains(SECRET));
}

#[test]
fn a_locked_value_refuses_every_edit_and_says_why() {
    let mut roots = tree();
    let s = roots[0].children[0].setting.take().unwrap().lock("the file fails closed");
    roots[0].children[0].setting = Some(s);
    roots[0].children[0].finding = Some("a.yaml:3: does not parse".into());
    let mut app = App::new("t", roots).adapter(Probe::default());
    for k in [KeyCode::Enter, KeyCode::Char('e'), KeyCode::Char('d'), KeyCode::Char(' ')] {
        press(&mut app, &[k]);
        assert!(app.message().starts_with("locked: the file fails closed"), "{k:?}: {}", app.message());
        assert!(matches!(app.mode, Mode::Browse) && app.pending() == 0);
    }
    let f = text(&render(&mut app, 100, 20));
    assert!(f.lines().any(|l| l.contains("tau") && l.contains(" !")), "the row is marked:\n{f}");
    assert!(f.contains("finding  a.yaml:3: does not parse") && f.contains("locked   the file fails closed"), "{f}");
}

#[test]
fn a_reload_keeps_pending_values_and_open_groups_and_names_a_moved_one() {
    let (mut app, p) = probe();
    // tau edited to 0.4; floor's file changes under no edit.
    press(&mut app, &[KeyCode::Char('e')]);
    app.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    type_str(&mut app, "0.4");
    press(&mut app, &[KeyCode::Enter]);
    let mut fresh = tree();
    fresh[0].open = false;
    fresh[0].children[1].setting.as_mut().unwrap().loaded = "0.25".into();
    fresh[0].children[1].setting.as_mut().unwrap().value = "0.25".into();
    *p.fresh.borrow_mut() = Some(fresh.clone());
    app.reload();
    let s = |app: &App, i: usize| app.roots[0].children[i].setting.clone().unwrap();
    assert_eq!((s(&app, 0).loaded.as_str(), s(&app, 0).value.as_str()), ("0.5", "0.4"), "the pending edit is kept");
    assert_eq!(s(&app, 1).value, "0.25", "an outside change shows");
    assert!(app.roots[0].open, "what was open stays open");
    assert!(!app.message().contains("changed on disk"));
    // tau's own file moves under its pending edit: the edit stays, and the message says so.
    fresh[0].children[0].setting.as_mut().unwrap().loaded = "0.6".into();
    fresh[0].children[0].setting.as_mut().unwrap().value = "0.6".into();
    *p.fresh.borrow_mut() = Some(fresh);
    let r = app.reload();
    assert_eq!((s(&app, 0).loaded.as_str(), s(&app, 0).value.as_str()), ("0.6", "0.4"));
    assert_eq!(r.moved, ["matching.tau"]);
    assert!(r.message().contains("matching.tau changed on disk under a pending edit; the edit is kept"), "{}", r.message());
}

#[test]
fn a_reload_that_cannot_keep_an_edit_says_it_was_dropped_and_why() {
    let (mut app, p) = probe();
    // Edits to tau and floor; then tau's file breaks, so tau comes back
    // locked, and floor is gone from the settings.
    press(&mut app, &[KeyCode::Char('e')]);
    app.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    type_str(&mut app, "0.4");
    press(&mut app, &[KeyCode::Enter, KeyCode::Down, KeyCode::Char('e')]);
    app.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    type_str(&mut app, "0.3");
    press(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.pending(), 2);
    let mut fresh = tree();
    let s = fresh[0].children[0].setting.take().unwrap().lock("a.yaml does not parse");
    fresh[0].children[0].setting = Some(s);
    fresh[0].children.remove(1);
    *p.fresh.borrow_mut() = Some(fresh);
    p.stamp.set(1);
    app.watch();
    assert_eq!(app.pending(), 0, "neither edit could be kept");
    assert_eq!(
        app.message(),
        "2 pending edits dropped: matching.tau: a.yaml does not parse; matching.floor: it is gone from the settings",
        "the message says what was dropped, not that review shows both"
    );
}

#[test]
fn a_reload_finds_the_cursor_closed_groups_and_failure_by_key() {
    // The fresh tree has rows the old one did not, above the cursor.
    let (mut app, p) = probe();
    press(&mut app, &[KeyCode::Down]);
    assert_eq!(tree::key(&app.roots, &app.rows()[app.cursor].path), "matching.floor");
    let mut fresh = tree();
    // A findings group now heads the tab, as one does when a file gains a finding.
    fresh[0].children.insert(0, Node::group("findings", "", vec![Node::leaf("#1", "", Setting::new(Kind::ReadOnly, "x", "user"))]).opened());
    *p.fresh.borrow_mut() = Some(fresh);
    app.reload();
    assert_eq!(tree::key(&app.roots, &app.rows()[app.cursor].path), "matching.floor", "the cursor stays on its key");
}

#[test]
fn a_view_action_switches_through_the_adapter_reloads_and_queues_nothing() {
    let with_view = || {
        let mut t = tree();
        t[0].actions = vec![Action::new("wide", "view: everything").arg(Arg::View("wide".into()))];
        t
    };
    let p = Probe::default();
    let mut app = App::new("t", with_view()).adapter(p.clone());
    press(&mut app, &[KeyCode::Down, KeyCode::Char('e')]);
    app.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    type_str(&mut app, "0.3");
    press(&mut app, &[KeyCode::Enter]);
    let mut fresh = with_view();
    fresh[0].children.insert(0, Node::leaf("more", "", Setting::new(Kind::ReadOnly, "x", "user")));
    *p.fresh.borrow_mut() = Some(fresh);
    let key = tree::action_keys(&app.roots[0].actions)[0].unwrap();
    press(&mut app, &[KeyCode::Char(key)]);
    assert_eq!(*p.views.borrow(), ["wide"], "the key asked the adapter for the view");
    assert_eq!(p.reloads.get(), 1, "then the tree reloaded");
    assert!(app.queued().is_empty(), "a view queues nothing");
    assert_eq!(app.pending(), 1, "the pending edit came through");
    assert_eq!(app.roots[0].children[2].setting.as_ref().unwrap().value, "0.3");
    assert_eq!(tree::key(&app.roots, &app.rows()[app.cursor].path), "matching.floor", "the cursor stays on its key");
    assert_eq!(app.message(), "showing wide");
    assert_eq!(*p.seen_pending.borrow(), [vec!["matching.floor".to_string()]], "the adapter saw the pending edit's store");
    assert!(text(&render(&mut app, 100, 20)).contains("matching · 1 views"), "the tree pane's title is the adapter's for the tab");
    app.watch();
    assert_eq!(p.reloads.get(), 1, "the files the view reads count as read");
    // The menu picks it too.
    press(&mut app, &[KeyCode::Char('a')]);
    assert!(text(&render(&mut app, 100, 20)).contains("view"), "the menu tags it");
    press(&mut app, &[KeyCode::Enter]);
    assert_eq!(p.views.borrow().len(), 2);
    assert!(app.queued().is_empty() && matches!(app.mode, Mode::Browse));
}

#[test]
fn a_view_the_adapter_refuses_changes_nothing_and_says_why() {
    let mut t = tree();
    t[0].actions = vec![Action::new("wide", "view: everything").arg(Arg::View("wide".into()))];
    let p = Probe::default();
    *p.fresh.borrow_mut() = Some(t.clone());
    *p.refuse.borrow_mut() = Some("1 pending change would vanish".into());
    let mut app = App::new("t", t).adapter(p.clone());
    press(&mut app, &[KeyCode::Char('e')]);
    app.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    type_str(&mut app, "0.4");
    press(&mut app, &[KeyCode::Enter]);
    let key = tree::action_keys(&app.roots[0].actions)[0].unwrap();
    press(&mut app, &[KeyCode::Char(key)]);
    assert_eq!(app.message(), "refused: 1 pending change would vanish");
    assert_eq!(p.reloads.get(), 0, "a refused view reloads nothing");
    assert!(p.views.borrow().is_empty() && app.pending() == 1 && app.queued().is_empty());
}

#[test]
fn the_masked_entry_shows_dots_while_a_secret_is_typed() {
    const SECRET: &str = "§¤ß¥¶";
    let (mut app, _) = probe();
    press(&mut app, &[KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
    assert!(app.masked());
    for (i, c) in SECRET.chars().enumerate() {
        press(&mut app, &[KeyCode::Char(c)]);
        let f = text(&render(&mut app, 100, 20));
        let dots = "•".repeat(i + 1);
        assert!(f.contains("secret") && f.contains(&format!(" {dots}▏")), "one dot per character after {} characters:\n{f}", i + 1);
        assert!(!f.contains(&format!("{dots}•")), "no more dots than characters:\n{f}");
        assert!(!f.chars().any(|x| SECRET.contains(x)), "a typed character reached the frame:\n{f}");
    }
}

#[test]
fn a_long_secret_shows_its_dots_up_to_a_cap_then_its_length() {
    assert_eq!(super::render::mask(0), "");
    assert_eq!(super::render::mask(3), "•••");
    assert_eq!(super::render::mask(100), format!("{}… 100", "•".repeat(24)));
}

/// A job that runs until stopped.
struct Forever {
    stopped: Rc<Cell<bool>>,
}

impl crate::adapter::Job for Forever {
    fn poll(&mut self) -> Option<Result<(), String>> {
        self.stopped.get().then(|| Err("killed".into()))
    }
    fn stop(&mut self) {
        self.stopped.set(true);
    }
}

#[derive(Default, Clone)]
struct Slow {
    stopped: Rc<Cell<bool>>,
    reloads: Rc<Cell<usize>>,
}

impl Adapter for Slow {
    fn write(&mut self, _: &Path, _: &[Write]) -> Result<(), String> {
        Ok(())
    }
    fn run(&mut self, _: &Queued) -> Result<(), String> {
        unreachable!("started, never run to the end")
    }
    fn start(&mut self, _: &Queued) -> Box<dyn crate::adapter::Job> {
        Box::new(Forever { stopped: self.stopped.clone() })
    }
    fn reload(&mut self) -> Option<Vec<Node>> {
        self.reloads.set(self.reloads.get() + 1);
        None
    }
    fn stamp(&self) -> Option<u64> {
        Some(1)
    }
}

/// One tab whose group has a reading `check` (action 0) and a queued
/// `rotate` (action 1), on an adapter whose commands run until stopped.
fn checking() -> (App, Slow) {
    let slow = Slow::default();
    let actions = vec![Action::new("check", "probe check").reads(), Action::new("rotate", "probe rotate")];
    let roots = vec![Node::group("keys", "", vec![Node::leaf("anthropic", "", Setting::new(Kind::Text, "present", "computed"))]).with_actions(actions)];
    (App::new("t", roots).adapter(slow.clone()), slow)
}

#[test]
fn a_check_in_flight_keeps_the_screen_live_and_queues_nothing() {
    let (mut app, slow) = checking();
    app.pick(vec![0], 0);
    for _ in 0..5 {
        app.tick();
    }
    assert!(app.applying(), "the check is running, so the loop ticks it");
    assert_eq!(app.pending(), 0, "nothing was queued");
    assert_eq!(app.message(), "check: running…");
    assert_eq!(slow.reloads.get(), 0);
}

#[test]
fn a_second_check_while_one_runs_is_refused() {
    let (mut app, _) = checking();
    app.pick(vec![0], 0);
    app.pick(vec![0], 0);
    assert_eq!(app.message(), "a check is running");
    assert!(app.applying());
}

#[test]
fn an_apply_waits_for_a_running_check() {
    let (mut app, slow) = checking();
    app.pick(vec![0], 1);
    assert_eq!(app.pending(), 1);
    app.pick(vec![0], 0);
    press(&mut app, &[KeyCode::Char('w'), KeyCode::Char('a')]);
    assert_eq!(app.message(), "a check is running");
    assert!(matches!(app.mode, Mode::Review { run: None, .. }), "no run started");
    assert!(!slow.stopped.get() && app.pending() == 1);
}

#[test]
fn stopping_the_run_stops_a_running_check() {
    let (mut app, slow) = checking();
    app.pick(vec![0], 0);
    app.stop_run("stopped by a signal");
    assert!(slow.stopped.get() && !app.applying());
}

#[test]
fn ctrl_c_during_a_check_with_nothing_pending_quits() {
    let (mut app, _) = checking();
    app.pick(vec![0], 0);
    assert!(!app.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)), "the session ends");
    assert!(!matches!(app.mode, Mode::Guard { .. }), "nothing pending, so no guard");
}

#[test]
fn a_check_ending_under_an_open_menu_marks_the_tree_unread() {
    let (mut app, slow) = checking();
    app.pick(vec![0], 0);
    press(&mut app, &[KeyCode::Char('?')]);
    assert!(matches!(app.mode, Mode::Help { .. }));
    slow.stopped.set(true);
    app.tick();
    assert!(!app.applying());
    assert_eq!(app.message(), "check: killed");
    assert_eq!(slow.reloads.get(), 0, "nothing is read under the open overlay");
    assert_eq!(app.stamp, None, "the next watch reads the tree");
    // Closing the help opens the check's error, held under it; closing
    // that leaves the screen browsing, where the owed read happens.
    press(&mut app, &[KeyCode::Esc]);
    assert!(matches!(app.mode, Mode::Response(_)), "the held error opens as the help closes");
    app.watch();
    assert_eq!(slow.reloads.get(), 0, "nothing is read under the modal either");
    press(&mut app, &[KeyCode::Esc]);
    app.watch();
    assert_eq!(app.message(), "check: killed", "the owed reload keeps the check's outcome");
    assert_eq!(slow.reloads.get(), 1);
}

#[test]
fn a_check_ending_in_browse_reads_the_tree_again() {
    let (mut app, slow) = checking();
    app.pick(vec![0], 0);
    slow.stopped.set(true);
    app.tick();
    assert_eq!(slow.reloads.get(), 1);
    assert_eq!(app.message(), "check: killed");
}

#[test]
fn a_command_in_flight_keeps_the_screen_live_and_a_second_ctrl_c_stops_it() {
    let slow = Slow::default();
    let mut app = App::new("t", tree()).adapter(slow.clone());
    app.queue.push(Queued::new("matching", "check", "ways agent key check", false));
    press(&mut app, &[KeyCode::Char('w'), KeyCode::Char('a')]);
    for _ in 0..10 {
        app.tick();
    }
    assert!(app.applying(), "the command is still running");
    let _ = render(&mut app, 100, 20);
    let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(app.key(ctrl_c));
    assert!(app.applying() && app.message().contains("^C again stops it"));
    assert!(app.key(ctrl_c));
    assert!(!app.applying() && slow.stopped.get(), "the second ^C ends the command");
    assert!(app.message().contains("stopped at step 1 of 1: stopped by ^C"), "{}", app.message());
    assert_eq!(slow.reloads.get(), 1, "a failed command is followed by a read of the files");
    assert_eq!(app.queued().len(), 1, "the command stays queued");
}

#[test]
fn a_signal_stops_a_command_in_flight() {
    let slow = Slow::default();
    let mut app = App::new("t", tree()).adapter(slow.clone());
    app.queue.push(Queued::new("matching", "check", "ways agent key check", false));
    press(&mut app, &[KeyCode::Char('w'), KeyCode::Char('a')]);
    app.tick();
    app.tick();
    app.stop_run("stopped by a signal");
    assert!(slow.stopped.get() && !app.applying());
}

#[test]
fn a_reload_with_the_theme_tab_open_keeps_the_app_alive() {
    let (app, p) = probe();
    let mut app = app.themes(Themes::new(None, agent_theme::ColorDepth::TrueColor, None));
    // Tab 2 is the theme tab: the one past the single settings tab.
    press(&mut app, &[KeyCode::Char('2')]);
    assert_eq!(app.tab(), 1);
    // Choose a theme, then let the watch see a file change, as the write
    // of the choice makes.
    press(&mut app, &[KeyCode::Down]);
    *p.fresh.borrow_mut() = Some(tree());
    p.stamp.set(9);
    app.watch();
    assert_eq!(p.reloads.get(), 1);
    app.reload();
    let _ = render(&mut app, 100, 20);
    assert_eq!(app.tab(), 1, "the theme tab is still shown");
}

#[test]
fn a_secret_past_the_cap_is_refused_out_loud() {
    let (mut app, _) = probe();
    press(&mut app, &[KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
    for _ in 0..crate::tree::SecretBuf::CAP {
        press(&mut app, &[KeyCode::Char('k')]);
    }
    assert!(!app.message().starts_with("rejected"));
    press(&mut app, &[KeyCode::Char('x')]);
    assert!(app.message().contains("holds at most 512 bytes"), "{}", app.message());
    assert!(app.masked(), "the entry stays open");
}

#[test]
fn a_new_app_draws_plain_lozenges() {
    let mut app = App::new("t", tree());
    let f = text(&render(&mut app, 100, 20));
    assert!(!f.contains('\u{e0b6}') && !f.contains('\u{e0b4}'), "no Nerd Font glyphs by default:\n{f}");
}

#[test]
fn the_watch_reloads_when_the_stamp_moves() {
    let (mut app, p) = probe();
    *p.fresh.borrow_mut() = Some(tree());
    app.watch();
    assert_eq!(p.reloads.get(), 0, "an unmoved stamp reads nothing");
    p.stamp.set(7);
    app.watch();
    assert_eq!(p.reloads.get(), 1);
    assert!(app.message().contains("reloaded"));
    app.watch();
    assert_eq!(p.reloads.get(), 1, "once per change");
}

#[test]
fn the_help_overlay_lists_the_keys_then_the_tabs_help_and_scrolls() {
    let (mut app, _) = probe();
    press(&mut app, &[KeyCode::Char('?')]);
    let f = text(&render(&mut app, 100, 40));
    assert!(f.contains("help: matching") && f.contains("matching: what this tab holds") && f.contains("a line of matching help"), "{f}");
    assert!(f.contains("Tab S-Tab 1-9"), "the keys come first");
    press(&mut app, &[KeyCode::Down, KeyCode::Down]);
    let small = text(&render(&mut app, 100, 12));
    assert!(!small.contains("Tab S-Tab 1-9"), "scrolled past the first lines:\n{small}");
    press(&mut app, &[KeyCode::Char('x')]);
    assert!(matches!(app.mode, Mode::Browse), "any other key closes it");
}

#[test]
fn with_no_adapter_an_apply_writes_nothing_and_stops() {
    let mut app = App::new("t", tree());
    press(&mut app, &[KeyCode::Char('e')]);
    type_str(&mut app, "");
    app.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    type_str(&mut app, "0.4");
    press(&mut app, &[KeyCode::Enter, KeyCode::Char('w'), KeyCode::Char('a')]);
    finish_apply(&mut app);
    assert!(app.message().contains("stopped at step 1 of 1"), "{}", app.message());
    assert_eq!(app.pending(), 1);
    let f = text(&render(&mut app, 110, 24));
    assert!(f.contains("no adapter: /c/a.yaml was not written"), "{f}");
}

#[test]
fn tabs_open_where_asked_and_name_themselves() {
    let app = App::new("t", tree()).on_tab(1);
    assert_eq!((app.tab(), app.tab_names()), (1, vec!["matching".to_string(), "theme".to_string()]));
    assert_eq!(App::new("t", tree()).on_tab(9).tab(), 0, "a tab past the last is ignored");
}

#[test]
fn a_section_is_no_part_of_its_rows_keys_and_a_reload_keeps_its_state() {
    let sectioned = || {
        let mut t = tree();
        let rows = std::mem::take(&mut t[0].children);
        t[0].children = vec![Node::section("settings", "", ("", "value"), rows)];
        t
    };
    let p = Probe::default();
    let mut app = App::new("t", sectioned()).adapter(p.clone());
    // The rows under the section keep the keys they had without it.
    assert_eq!(tree::key(&app.roots, &[0, 0, 0]), "matching.tau");
    assert_eq!(tree::key(&app.roots, &[0, 0]), "matching#settings");
    assert_eq!(tree::label(&app.roots, &[0, 0]), "matching · settings");
    assert_eq!(tree::path_of(&app.roots, "matching.floor"), Some(vec![0, 0, 1]));
    // Edit tau (first row under the section), close the section, reload.
    press(&mut app, &[KeyCode::Down, KeyCode::Char('e')]);
    app.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    type_str(&mut app, "0.4");
    press(&mut app, &[KeyCode::Enter]);
    assert_eq!(tree::changes(&app.roots)[0].0, "matching.tau", "a change's key carries no section");
    app.roots[0].children[0].open = false;
    *p.fresh.borrow_mut() = Some(sectioned());
    let r = app.reload();
    assert!(r.is_clean(), "{}", r.message());
    assert_eq!(app.roots[0].children[0].children[0].setting.as_ref().unwrap().value, "0.4", "the pending edit is kept");
    assert!(!app.roots[0].children[0].open, "the section's open state is kept");
}
