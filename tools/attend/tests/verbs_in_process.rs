//! The message verbs called as a library, in this test process, the way a
//! long-lived frontend (an MCP server, a mod) calls them (ADR-501 item 4).
//!
//! The verbs read HOME and the XDG roots from the environment, which is
//! process-wide, so each test holds [`LOCK`] while it points them at its
//! own fixture.

mod common;
use common::Fixture;
use std::sync::{Mutex, MutexGuard};

static LOCK: Mutex<()> = Mutex::new(());

/// Point this process at `f`'s sandbox for the life of the guard.
fn enter(f: &Fixture) -> MutexGuard<'static, ()> {
    let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("HOME", &f.home);
    std::env::set_var("XDG_CACHE_HOME", f.home.join(".cache"));
    std::env::set_var("XDG_CONFIG_HOME", f.home.join("config"));
    std::env::set_var("XDG_DATA_HOME", f.home.join("data"));
    std::env::remove_var("CLAUDE_SESSION_ID");
    guard
}

fn members(f: &Fixture, channel: &str) -> Vec<String> {
    attend_groups::load_groups(&f.signals()).get(channel).map(|e| e.members.clone()).unwrap_or_default()
}

/// `/clear` gives a running session a new id. A frontend that stays up across
/// it must act as the new id on its next call, and use that one id for every
/// part of the call: the enrollment and the membership both.
#[test]
fn a_join_after_an_id_change_enrolls_and_joins_as_the_new_id() {
    let f = Fixture::new("inproc-idchange");
    let _env = enter(&f);
    let old = f.sid.clone();

    attend::verbs::channels::join("before", false).expect("join as the first id");
    assert!(f.marker(&old).exists(), "the first join enrolls the first id");
    assert_eq!(members(&f, "before"), vec![old.clone()]);

    let new = "sess-inproc-after-clear";
    f.set_session_id(new);
    attend::verbs::channels::join("after", false).expect("join as the new id");
    assert!(f.marker(new).exists(), "the second join enrolls the new id");
    assert_eq!(members(&f, "after"), vec![new.to_string()], "and writes its membership under it");
}
