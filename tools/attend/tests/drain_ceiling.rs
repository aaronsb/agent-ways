//! The Stop-hook re-entry ceiling fails closed (#725 re-review, finding 7).
//!
//! The round counter lives in the state directory. When that directory
//! cannot be written, the counter never persists, so it would read 1 on
//! every hook-forced continuation and the ceiling would never trip: an
//! old backlog's note would block every Stop. A hook-forced drain that
//! cannot record its round defers to the Monitor instead.

mod common;
use common::{Fixture, MINUTE};

#[cfg(unix)]
#[test]
fn an_unwritable_state_dir_cannot_loop_the_stop_hook() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new("ceiling");
    f.ok(&["join", "side"]);
    f.put("_broadcast", "other-1-old", "old backlog", 30 * MINUTE);
    let state = f.cache().join("state");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o555)).unwrap();

    // A fresh boundary may block once, to announce the backlog.
    f.drain_hook_with(r#"{"stop_hook_active": false}"#);
    // A continuation forced by that block must not block again.
    let again = f.drain_hook_with(r#"{"stop_hook_active": true}"#);
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(again, "", "a forced continuation that cannot count its round must end the turn");
}
